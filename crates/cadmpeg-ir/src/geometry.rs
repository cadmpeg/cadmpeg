// SPDX-License-Identifier: Apache-2.0
//! Geometry carriers: analytic surfaces, analytic curves, NURBS, and pcurves.
//!
//! Carriers are stored in their own arenas and referenced by id from the
//! topology graph (a face references a [`Surface`], an edge a [`Curve`], a
//! coedge a [`Pcurve`]). One carrier may therefore support several topological
//! entities.

use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;
use crate::transform::Transform;
use crate::units::{FiniteScalar, FiniteVector, NonNegativeScalar};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroI64;

/// Checked procedural curve payloads.
pub mod curve_payloads;
/// Checked procedural surface payloads.
pub mod surface_payloads;

fn default_true() -> bool {
    true
}

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tertiary: Option<bool>,
    },
}

/// Mutually exclusive pre-revision and revision-gated offset layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "layout", rename_all = "snake_case", deny_unknown_fields)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
pub enum OffsetExtension {
    /// Pre-revision conditional flag sequence.
    Legacy {
        /// Conditional flag sequence in its positional wire form.
        flags: LegacyExtensionFlags,
        /// Solved-cache fit contract this layout states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated fields with the required four-boolean carrier run.
    Revision {
        /// Revision-gated form whose carrier run is exactly four booleans.
        form: RevisionSurfaceForm<[bool; 4]>,
    },
}

mod carriers;
pub use carriers::{
    knots_nondecreasing, knots_strictly_increasing, BsplineSurface, CircleCurve, CirclePcurve,
    ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, EllipsePcurve,
    GeometryLayoutError, HarmonicPcurve, HyperbolaCurve, HyperbolaPcurve, HyperbolicPcurve,
    LineCurve, LinePcurve, NurbsCurve, NurbsError, NurbsSurface, OffsetPcurve, ParabolaCurve,
    ParabolaPcurve, Pcurve, PcurveGeneralForm, PcurveGeometry, PcurveInlineForm, PcurveMetadata,
    PcurveNurbs, PlaneSurface, PolarHarmonicPcurve, PolarNurbsPole, PolarPcurveNurbs,
    PolygonalSurface, PolylineCurve, SphereSurface, SphericalGreatCirclePcurve,
    SurfaceParameterAxis, TorusSurface, TrimmedPcurve,
};

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
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<SolvedSurfaceGeometry>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
        self_intersect: Option<bool>,
    },
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
    /// Source-native polyline with an explicit chordal error bound.
    Polyline(PolylineCurve),
    /// Exact affine placement of an inline basis curve.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<SolvedCurveGeometry>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
    /// Native curve carrier whose shape is not decoded.
    Unknown {
        /// Retained native record containing the curve carrier.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
    if !norm.is_finite() || norm == 0.0 {
        return Vector3::new(1.0, 0.0, 0.0);
    }
    let axis = Vector3::new(axis.x / norm, axis.y / norm, axis.z / norm);
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
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
    pub record_bounds: Option<[Option<f64>; 4]>,
}

/// Parameter fields carried by exact and loft spline-surface constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SplineSurfaceParameters {
    /// Ordered semantic U and V intervals in the legacy layout.
    OrderedRanges {
        /// Ordered U and V intervals.
        ranges: [[f64; 2]; 2],
    },
    /// Two parameter intervals in a revision-gated layout, each stored as an
    /// ordered `[lo, hi]` pair of optional bounds. For exact and t-spline
    /// surfaces these are the surface's unextended (pre-extension) parameter
    /// ranges; for loft surfaces they are wrap ranges, where a reversed pair
    /// (`lo > hi`) encodes an empty interval (no wrap). `None` is a false
    /// bound-presence flag.
    RevisionRanges {
        /// Two parameter intervals in serialized field order.
        intervals: [[Option<f64>; 2]; 2],
    },
}

/// Mutually exclusive legacy and revision-gated exact-spline layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "layout", rename_all = "snake_case", deny_unknown_fields)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
pub enum ExactSpline {
    /// Legacy solved-cache layout with ordered U/V ranges.
    Legacy {
        /// Ordered U and V parameter ranges.
        ranges: [[f64; 2]; 2],
        /// Native ASM extension integer following the ranges.
        extension: i64,
        /// Solved-cache fit contract this layout states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated layout with optional interval bounds and shared form.
    Revision {
        /// Two optional-bound parameter intervals in wire order.
        intervals: [[Option<f64>; 2]; 2],
        /// Native ASM extension enum following the intervals.
        extension: i64,
        /// Required revision-gated form.
        form: RevisionSurfaceForm,
    },
}

/// One component and its native construction scalar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompoundComponent<T> {
    /// Scalar paired with this component.
    pub parameter: f64,
    /// Component geometry or its resolved identity.
    pub component: T,
}

/// A non-empty compound curve with finite construction parameters.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CompoundCurveConstructionWire")]
pub struct CompoundCurveConstruction {
    parameters: Vec<f64>,
    components: Vec<CompoundComponent<CurveId>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<LegacyCache>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CompoundCurveConstructionWire {
    parameters: Vec<f64>,
    components: Vec<CompoundComponent<CurveId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<LegacyCache>,
}

impl TryFrom<CompoundCurveConstructionWire> for CompoundCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: CompoundCurveConstructionWire) -> Result<Self, Self::Error> {
        let mut payload = Self::try_new(wire.parameters, wire.components)?;
        payload.cache = wire.cache;
        Ok(payload)
    }
}

impl Serialize for CompoundCurveConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CompoundCurveConstructionWire {
            parameters: self.parameters.clone(),
            components: self.components.clone(),
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
    ) -> Result<Self, &'static str> {
        if components.is_empty() {
            return Err("compound curve components must not be empty");
        }
        if parameters
            .iter()
            .chain(components.iter().map(|item| &item.parameter))
            .any(|value| !value.is_finite())
        {
            return Err("compound curve parameters must be finite");
        }
        Ok(Self {
            cache: None,
            parameters,
            components,
        })
    }

    /// Return the parameters.
    #[must_use]
    pub fn parameters(&self) -> &[f64] {
        &self.parameters
    }

    /// Return the components.
    #[must_use]
    pub fn components(&self) -> &[CompoundComponent<CurveId>] {
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    Blend(surface_payloads::BlendSurfacePayload),
    RollingBallJet(RollingBallJetStations),
    Unknown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
}

impl<'de> Deserialize<'de> for ProceduralSurfaceDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ProceduralSurfaceDefinitionWire::deserialize(deserializer)
    }
}

impl ProceduralSurfaceDefinition {
    fn revision_cache(&self) -> Option<&RevisionCacheForm> {
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

    fn revision_cache_mut(&mut self) -> Option<&mut RevisionCacheForm> {
        match self {
            Self::Exact(payload) => payload.revision_cache_mut(),
            Self::Taper(payload) => payload.revision_cache_mut(),
            Self::Extrusion(payload) => payload.revision_cache_mut(),
            Self::Revolution(payload) => payload.revision_cache_mut(),
            Self::Sum(payload) => payload.revision_cache_mut(),
            Self::Offset(payload) => payload.revision_cache_mut(),
            Self::Loft(payload) => payload.revision_cache_mut(),
            Self::RevisionCompoundLoft { construction } => Some(&mut construction.cache),
            Self::RevisionG2Blend { construction } => Some(&mut construction.cache),
            Self::Sweep(payload) => payload.revision_cache_mut(),
            Self::TSpline { construction } => {
                construction.cache.form_mut().map(|form| &mut form.cache)
            }
            Self::Deformable(payload) => payload.revision_cache_mut(),
            Self::Blend(payload) => payload.revision_cache_mut(),
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

    /// Whether the construction owns a revision-gated cache form, which then
    /// states its solved-cache fit tolerance.
    #[must_use]
    pub fn owns_revision_cache(&self) -> bool {
        self.revision_cache().is_some() || matches!(self, Self::VariableBlend(..))
    }
}

/// A finite, non-negative fit tolerance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "f64", into = "f64")]
pub struct FitTolerance(f64);

impl FitTolerance {
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

    /// A legacy layout carrying `cache`.
    #[must_use]
    pub const fn legacy_cache(cache: Option<LegacyCache>) -> Self {
        Self::Legacy { cache }
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
    pub(crate) const fn legacy_cache_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
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
pub(crate) enum LegacyCacheSlot<'a> {
    /// Slot of a layout whose record may state no cache.
    Optional(&'a mut Option<LegacyCache>),
    /// Slot of a layout that always states a cache.
    Required(&'a mut LegacyCache),
}

impl LegacyCacheSlot<'_> {
    /// Replace the cache the slot holds.
    pub(crate) const fn set(
        &mut self,
        cache: Option<LegacyCache>,
    ) -> Result<(), CacheContractError> {
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
            return payload
                .construction()
                .cache
                .fit_tolerance()
                .map(FitTolerance);
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
            return set_variable_blend_cache(payload.cache_mut(), value);
        }
        if self.owns_revision_cache() {
            return set_revision_cache(self.revision_cache_mut(), value);
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
            return set_revision_cache(self.revision_cache_mut(), value);
        }
        match value {
            Some(value) => self.set_legacy_cache(LegacyCache::new(value)),
            None => {
                self.clear_legacy_cache();
                Ok(())
            }
        }
    }

    /// Raise the fit tolerance of an existing solved cache.
    ///
    /// A read-modify route, never a write route: a form with no solved cache
    /// stays without one. Parameterized forms have no solved cache and remain
    /// unchanged, and so do a form whose layout states no legacy slot and a
    /// form whose legacy slot is empty.
    pub fn raise_cache_fit_tolerance(&mut self, value: FitTolerance) {
        match self.revision_cache_mut() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => {
                if value.get() > fit_tolerance.get() {
                    *fit_tolerance = value;
                }
            }
            Some(RevisionCacheForm::Parameterization(_)) => {}
            None => {
                if let Some(Some(cache)) = self.legacy_cache_slot_mut() {
                    if value.get() > cache.fit_tolerance.get() {
                        cache.fit_tolerance = value;
                    }
                }
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
        match self.revision_cache_mut() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => {
                if value.get() > fit_tolerance.get() {
                    *fit_tolerance = value;
                }
                Ok(())
            }
            Some(RevisionCacheForm::Parameterization(_)) => {
                Err(CacheContractError::Layout(PARAMETERIZED_NO_SOLVED_CACHE))
            }
            None => match self.legacy_cache_slot_mut() {
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
    cache: &mut VariableBlendCache,
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

fn set_revision_cache<P>(
    cache: Option<&mut RevisionCacheForm<P>>,
    value: Option<FitTolerance>,
) -> Result<(), CacheContractError> {
    match (cache, value) {
        (Some(RevisionCacheForm::Parameterization(_)), Some(_)) => {
            Err(CacheContractError::Parameterized)
        }
        (Some(RevisionCacheForm::SolvedCache { fit_tolerance }), Some(value)) => {
            *fit_tolerance = value;
            Ok(())
        }
        (Some(RevisionCacheForm::SolvedCache { .. }), None) => {
            Err(CacheContractError::MissingSolved)
        }
        (Some(RevisionCacheForm::Parameterization(_)) | None, None) => Ok(()),
        (None, Some(_)) => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
    }
}

impl ProceduralSurface {
    /// Build a procedural surface from its construction definition.
    pub fn new(
        id: ProceduralSurfaceId,
        definition: ProceduralSurfaceDefinition,
        record_bounds: Option<[Option<f64>; 4]>,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self {
            id,
            definition,
            record_bounds,
        })
    }

    /// Borrow the neutral construction definition.
    #[must_use]
    pub fn definition(&self) -> &ProceduralSurfaceDefinition {
        &self.definition
    }

    /// Replace the construction definition. The cache contract travels with
    /// the definition, so nothing outside it changes.
    pub fn replace_definition(&mut self, definition: ProceduralSurfaceDefinition) {
        self.definition = definition;
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
    pub fn cache_fit_tolerance(&self) -> Option<f64> {
        self.definition.cache_fit_tolerance().map(FitTolerance::get)
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
    pub fn scale_cache_fit_tolerance(&mut self, scale: f64) -> Result<(), CacheContractError> {
        if let Some(value) = self.cache_fit_tolerance() {
            self.set_cache_fit_tolerance(Some(value * scale))?;
        }
        Ok(())
    }
}

/// Structurally selected deformable-surface payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeformableSurfaceData {
    /// Mode-6 full embedded deformation payload.
    Full {
        /// Four leading deformation vectors.
        leading_vectors: [Vector3; 4],
        /// Leading deformation scalar.
        leading_parameter: f64,
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
        first_parameter: f64,
        /// Version-gated ASM long when present.
        version_value: Option<i64>,
        /// Second scalar after the optional long.
        second_parameter: f64,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Two ordered full vector frames.
        frames: Box<[DeformableVectorFrame; 2]>,
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
        first_parameter: f64,
        /// Native selector integer.
        selector: i64,
        /// Second native scalar.
        second_parameter: f64,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Four ordered deformation vectors.
        vectors: [Vector3; 4],
        /// Frame scalar after the vectors.
        frame_parameter: f64,
        /// Three frame flags.
        flags: [bool; 3],
        /// Counted ordered scalar triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// Mode-1 deformation frame with counted parameter triples.
    Plain {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame>,
        /// Ordered native scalar triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// Mode-3 deformation frame with a guide scalar.
    Guided {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame>,
        /// Native guide selector.
        selector: i64,
        /// Native guide scalar.
        guide_parameter: f64,
    },
    /// Mode-8 minimal four-vector scaffold.
    Minimal {
        /// Four ordered deformation vectors.
        vectors: [Vector3; 4],
        /// Native trailing selector.
        selector: i64,
    },
    /// Revision-gated mode-3 deformation payload.
    RevisionMode3 {
        /// Four leading deformation vectors.
        leading_vectors: [Vector3; 4],
        /// Scalar following the leading vectors.
        leading_parameter: f64,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position anchoring the trailing frame.
        trailing_point: Point3,
        /// Two vectors following the trailing point.
        trailing_vectors: [Vector3; 2],
        /// Scalar following the trailing vectors.
        frame_parameter: f64,
        /// Two flags following the trailing frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalar parameters following the trailing frame.
        parameters: [f64; 3],
        /// Five flags following the ordered scalar parameters.
        trailing_flags: [bool; 5],
        /// Scalar preceding the payload's final integer.
        trailing_parameter: f64,
        /// Integer closing the revision mode-3 payload.
        trailing_value: i64,
    },
}

/// Four-vector frame used by full deformable surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableVectorFrame {
    /// Four ordered vectors.
    pub vectors: [Vector3; 4],
    /// Frame scalar.
    pub parameter: f64,
    /// Three ordered flags.
    pub flags: [bool; 3],
}

/// Shared frame payload of deformable-surface modes 1 and 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableSurfaceFrame {
    /// Four leading deformation vectors.
    pub leading_vectors: [Vector3; 4],
    /// Leading frame scalar.
    pub leading_parameter: f64,
    /// Three leading frame flags.
    pub leading_flags: [bool; 3],
    /// Three secondary deformation vectors.
    pub secondary_vectors: [Vector3; 3],
    /// Secondary frame scalar.
    pub secondary_parameter: f64,
    /// Two secondary frame flags.
    pub secondary_flags: [bool; 2],
    /// Native model-space frame point.
    pub point: Point3,
    /// Five trailing frame flags.
    pub trailing_flags: [bool; 5],
}

/// Complete native deformable-surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableSurfaceConstruction {
    /// Surface being deformed.
    pub support: SurfaceId,
    /// Discriminator-selected deformation data.
    pub data: DeformableSurfaceData,
    /// Cache contract: the revision-gated fields surrounding the support and
    /// shared surface tail, or the legacy solved-cache tolerance this
    /// construction states instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    pub cache: CacheContract<RevisionSurfaceForm>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
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
    apex_factor: FiniteScalar,
    axis: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixPathConstructionWire {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
    axis: Vector3,
}

impl HelixPathConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        center: Point3,
        major: Vector3,
        minor: Vector3,
        pitch: Vector3,
        apex_factor: f64,
        axis: Vector3,
    ) -> Result<Self, &'static str> {
        let major_length = (major.x.powi(2) + major.y.powi(2) + major.z.powi(2)).sqrt();
        let minor_length = (minor.x.powi(2) + minor.y.powi(2) + minor.z.powi(2)).sqrt();
        if !(major_length > 0.0
            && (major_length - minor_length).abs()
                <= EPS_HELIX_SURFACE_RADIUS_RELATIVE * major_length.max(1.0))
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
        let apex_factor = FiniteScalar::new(apex_factor)
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
    pub const fn angle_range(&self) -> &[f64; 2] {
        self.angle_range.as_raw()
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the major.
    #[must_use]
    pub const fn major(&self) -> &Vector3 {
        self.major.as_raw()
    }

    /// Return the minor.
    #[must_use]
    pub const fn minor(&self) -> &Vector3 {
        self.minor.as_raw()
    }

    /// Return the pitch.
    #[must_use]
    pub const fn pitch(&self) -> &Vector3 {
        self.pitch.as_raw()
    }

    /// Return the apex factor.
    #[must_use]
    pub const fn apex_factor(&self) -> f64 {
        self.apex_factor.get()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.axis.as_raw()
    }
}

impl TryFrom<HelixPathConstructionWire> for HelixPathConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixPathConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            wire.center,
            wire.major,
            wire.minor,
            wire.pitch,
            wire.apex_factor,
            wire.axis,
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
    apex_factor: FiniteScalar,
    axis: FiniteVector3,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<LegacyCache>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixCurveConstructionWire {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
    axis: Vector3,
    #[serde(default)]
    cache: Option<LegacyCache>,
}

impl HelixCurveConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        center: Point3,
        major: Vector3,
        minor: Vector3,
        pitch: Vector3,
        apex_factor: f64,
        axis: Vector3,
    ) -> Result<Self, &'static str> {
        if angle_range[0] > angle_range[1] {
            return Err("helix curve angle_range must be ordered");
        }
        if [major, minor, axis]
            .iter()
            .any(|vector| vector.norm() <= f64::EPSILON)
        {
            return Err("helix curve major, minor, and axis must be non-degenerate");
        }
        if (major.norm() - minor.norm()).abs() > EPS_HELIX_CURVE_RADIUS {
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
        let apex_factor = FiniteScalar::new(apex_factor)
            .ok_or("HelixCurveConstruction.apex_factor must be finite")?;
        let axis = FiniteVector3::new(axis).ok_or("HelixCurveConstruction.axis must be finite")?;
        Ok(Self {
            cache: None,
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
    pub const fn angle_range(&self) -> &[f64; 2] {
        self.angle_range.as_raw()
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the major.
    #[must_use]
    pub const fn major(&self) -> &Vector3 {
        self.major.as_raw()
    }

    /// Return the minor.
    #[must_use]
    pub const fn minor(&self) -> &Vector3 {
        self.minor.as_raw()
    }

    /// Return the pitch.
    #[must_use]
    pub const fn pitch(&self) -> &Vector3 {
        self.pitch.as_raw()
    }

    /// Return the apex factor.
    #[must_use]
    pub const fn apex_factor(&self) -> f64 {
        self.apex_factor.get()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.axis.as_raw()
    }
}

impl TryFrom<HelixCurveConstructionWire> for HelixCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixCurveConstructionWire) -> Result<Self, Self::Error> {
        let mut payload = Self::try_new(
            wire.angle_range,
            wire.center,
            wire.major,
            wire.minor,
            wire.pitch,
            wire.apex_factor,
            wire.axis,
        )?;
        payload.cache = wire.cache;
        Ok(payload)
    }
}

impl HelixCurveConstruction {
    /// Reverse the native interval and signed path fields.
    pub fn reverse_parameterization(&mut self) {
        self.angle_range = self.angle_range.reversed_negated();
        self.minor = self.minor.negated();
        self.pitch = self.pitch.negated();
        self.apex_factor = self.apex_factor.negated();
    }

    /// Scale lengths atomically and retain the old path when admission fails.
    pub fn try_scale_lengths(&mut self, scale: f64) -> Result<(), &'static str> {
        let vector =
            |value: Vector3| Vector3::new(value.x * scale, value.y * scale, value.z * scale);
        let candidate = Self::try_new(
            self.angle_range.get(),
            Point3::new(
                self.center.x * scale,
                self.center.y * scale,
                self.center.z * scale,
            ),
            vector(self.major.get()),
            vector(self.minor.get()),
            vector(self.pitch.get()),
            self.apex_factor.get(),
            self.axis.get(),
        )?;
        // The rebuilt construction is minted fresh; the solved-cache contract
        // this construction states travels with it.
        let cache = self.cache;
        *self = candidate;
        self.cache = cache;
        Ok(())
    }
}

/// Finite circular helix profile with a nonzero signed radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCircleProfileWire")]
pub struct HelixCircleProfile {
    length: FiniteScalar,
    radius: FiniteScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixCircleProfileWire {
    length: f64,
    radius: f64,
}

impl HelixCircleProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(length: f64, radius: f64) -> Result<Self, &'static str> {
        if radius == 0.0 {
            return Err("helix circle profile radius must be finite and nonzero");
        }
        let length =
            FiniteScalar::new(length).ok_or("helix circle profile length must be finite")?;
        let radius =
            FiniteScalar::new(radius).ok_or("helix circle profile radius must be finite")?;
        Ok(Self { length, radius })
    }
    /// Native profile length.
    #[must_use]
    pub const fn length(&self) -> f64 {
        self.length.get()
    }

    /// Signed circular profile radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
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
    direction: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixLineProfileWire {
    direction: Vector3,
}

impl HelixLineProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(direction: Vector3) -> Result<Self, &'static str> {
        if ![direction.x, direction.y, direction.z]
            .into_iter()
            .all(f64::is_finite)
            || direction.x * direction.x + direction.y * direction.y + direction.z * direction.z
                <= 0.0
        {
            return Err("helix line profile direction must be finite and non-degenerate");
        }
        Ok(Self { direction })
    }
    /// Finite non-degenerate profile direction.
    #[must_use]
    pub const fn direction(&self) -> Vector3 {
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
    angle_range: [f64; 2],
    dimension_range: [f64; 2],
    path: HelixPathConstruction,
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
    pub const fn angle_range(&self) -> &[f64; 2] {
        self.angle_range.as_raw()
    }

    /// Return the dimension range.
    #[must_use]
    pub const fn dimension_range(&self) -> &[f64; 2] {
        self.dimension_range.as_raw()
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
    pub program: crate::products::NonEmptyString,
    /// Optional native separator boolean.
    pub separator: Option<bool>,
    /// Companion values program.
    pub values: crate::products::NonEmptyString,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum InlineTSplineSubtransformWire {
    Inline {
        program: String,
        separator: Option<bool>,
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
            program: crate::products::NonEmptyString::new(program)
                .ok_or("T-spline program must not be empty")?,
            separator,
            values: crate::products::NonEmptyString::new(values)
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
    Inline {
        program: String,
        separator: Option<bool>,
        values: String,
    },
    Reference {
        index: SubtypeTableIndex,
        #[serde(default)]
        resolved: Option<Box<InlineTSplineSubtransform>>,
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
                transform: resolved.ok_or("T-spline subtransform is unresolved")?,
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

impl TSplineSurfaceConstruction {
    /// Admit finite ordered ranges, finite discontinuities, and a resolved subtransform.
    pub fn try_new(
        parameter_ranges: [[f64; 2]; 2],
        type_code: i64,
        subtransform: TSplineSubtransform,
        trailing_value: i64,
        discontinuities: [Vec<f64>; 6],
        discontinuity_flag: bool,
        revision_form: Option<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let parameter_ranges = [
            crate::topology::ParameterInterval::new(parameter_ranges[0])
                .map_err(ProceduralGeometryError::Payload)?,
            crate::topology::ParameterInterval::new(parameter_ranges[1])
                .map_err(ProceduralGeometryError::Payload)?,
        ];
        if !discontinuities
            .iter()
            .flatten()
            .all(|value| value.is_finite())
        {
            return Err(ProceduralGeometryError::Payload(
                "T-spline discontinuities must be finite",
            ));
        }
        Ok(Self {
            parameter_ranges,
            type_code,
            subtransform,
            trailing_value,
            discontinuities,
            discontinuity_flag,
            cache: CacheContract::from_form(revision_form),
        })
    }

    /// Return ordered U and V intervals.
    pub fn parameter_ranges(&self) -> [[f64; 2]; 2] {
        self.parameter_ranges
            .map(crate::topology::ParameterInterval::endpoints)
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
    pub const fn discontinuities(&self) -> &[Vec<f64>; 6] {
        &self.discontinuities
    }

    /// Return the native discontinuity flag value.
    pub const fn discontinuity_flag(&self) -> bool {
        self.discontinuity_flag
    }

    /// Return the native revision form value.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm> {
        self.cache.form()
    }

    /// Parse the semantic index of the effective topology program.
    #[must_use]
    pub fn program_graph(&self) -> TSplineProgram {
        TSplineProgram::parse(self.subtransform.inline().program.as_str())
    }

    /// Parse the semantic index of the effective values program.
    #[must_use]
    pub fn values_graph(&self) -> TSplineProgram {
        TSplineProgram::parse(self.subtransform.inline().values.as_str())
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "TSplineSurfaceConstruction"))]
#[serde(deny_unknown_fields)]
struct TSplineSurfaceConstructionWire {
    parameter_ranges: [[f64; 2]; 2],
    type_code: i64,
    subtransform: TSplineSubtransform,
    trailing_value: i64,
    discontinuities: [Vec<f64>; 6],
    discontinuity_flag: bool,
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm>,
}

impl From<TSplineSurfaceConstruction> for TSplineSurfaceConstructionWire {
    fn from(construction: TSplineSurfaceConstruction) -> Self {
        Self {
            parameter_ranges: construction.parameter_ranges(),
            type_code: construction.type_code,
            subtransform: construction.subtransform,
            trailing_value: construction.trailing_value,
            discontinuities: construction.discontinuities,
            discontinuity_flag: construction.discontinuity_flag,
            cache: construction.cache,
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
            wire.cache.form().cloned(),
        )
        .map(|mut construction| {
            construction.cache = wire.cache;
            construction
        })
    }
}

/// Leading token of a recognized T-spline header declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TSplineHeaderKind {
    /// `degree`
    #[serde(rename = "degree")]
    Degree,
    /// `cap_type`
    #[serde(rename = "cap_type")]
    CapType,
    /// `units`
    #[serde(rename = "units")]
    Units,
    /// `end_conditions`
    #[serde(rename = "end_conditions")]
    EndConditions,
    /// `star_knot_rule`
    #[serde(rename = "star_knot_rule")]
    StarKnotRule,
    /// `star_smoothness`
    #[serde(rename = "star_smoothness")]
    StarSmoothness,
    /// `tol`
    #[serde(rename = "tol")]
    Tol,
    /// `ver`
    #[serde(rename = "ver")]
    Ver,
    /// `behavior_version`
    #[serde(rename = "behavior_version")]
    BehaviorVersion,
    /// `geom_tol`
    #[serde(rename = "geom_tol")]
    GeomTol,
    /// `compat_version`
    #[serde(rename = "compat_version")]
    CompatVersion,
}

impl TSplineHeaderKind {
    /// Return the native leading token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Degree => "degree",
            Self::CapType => "cap_type",
            Self::Units => "units",
            Self::EndConditions => "end_conditions",
            Self::StarKnotRule => "star_knot_rule",
            Self::StarSmoothness => "star_smoothness",
            Self::Tol => "tol",
            Self::Ver => "ver",
            Self::BehaviorVersion => "behavior_version",
            Self::GeomTol => "geom_tol",
            Self::CompatVersion => "compat_version",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        Some(match token {
            "degree" => Self::Degree,
            "cap_type" => Self::CapType,
            "units" => Self::Units,
            "end_conditions" => Self::EndConditions,
            "star_knot_rule" => Self::StarKnotRule,
            "star_smoothness" => Self::StarSmoothness,
            "tol" => Self::Tol,
            "ver" => Self::Ver,
            "behavior_version" => Self::BehaviorVersion,
            "geom_tol" => Self::GeomTol,
            "compat_version" => Self::CompatVersion,
            _ => return None,
        })
    }
}

/// Leading token of a recognized T-spline topology, geometry, or constraint record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TSplineRecordKind {
    /// `f`
    #[serde(rename = "f")]
    Face,
    /// `e`
    #[serde(rename = "e")]
    Edge,
    /// `v`
    #[serde(rename = "v")]
    Vertex,
    /// `l`
    #[serde(rename = "l")]
    Link,
    /// `ec`
    #[serde(rename = "ec")]
    EdgeCondition,
    /// `0m`
    #[serde(rename = "0m")]
    Material0,
    /// `0g`
    #[serde(rename = "0g")]
    Geometry0,
    /// `100edges`
    #[serde(rename = "100edges")]
    Edges100,
    /// `100verts`
    #[serde(rename = "100verts")]
    Verts100,
    /// `105sym`
    #[serde(rename = "105sym")]
    Symmetry105,
    /// `105plane`
    #[serde(rename = "105plane")]
    Plane105,
    /// `105a`
    #[serde(rename = "105a")]
    A105,
    /// `106ek`
    #[serde(rename = "106ek")]
    EdgeKnots106,
    /// `50000grip`
    #[serde(rename = "50000grip")]
    Grip50000,
}

impl TSplineRecordKind {
    /// Return the native leading token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Face => "f",
            Self::Edge => "e",
            Self::Vertex => "v",
            Self::Link => "l",
            Self::EdgeCondition => "ec",
            Self::Material0 => "0m",
            Self::Geometry0 => "0g",
            Self::Edges100 => "100edges",
            Self::Verts100 => "100verts",
            Self::Symmetry105 => "105sym",
            Self::Plane105 => "105plane",
            Self::A105 => "105a",
            Self::EdgeKnots106 => "106ek",
            Self::Grip50000 => "50000grip",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        Some(match token {
            "f" => Self::Face,
            "e" => Self::Edge,
            "v" => Self::Vertex,
            "l" => Self::Link,
            "ec" => Self::EdgeCondition,
            "0m" => Self::Material0,
            "0g" => Self::Geometry0,
            "100edges" => Self::Edges100,
            "100verts" => Self::Verts100,
            "105sym" => Self::Symmetry105,
            "105plane" => Self::Plane105,
            "105a" => Self::A105,
            "106ek" => Self::EdgeKnots106,
            "50000grip" => Self::Grip50000,
            _ => return None,
        })
    }
}

/// Parsed line-oriented T-spline subtransform program.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TSplineProgram {
    headers: Vec<TSplineProgramLine<TSplineHeaderKind>>,
    records: Vec<TSplineProgramLine<TSplineRecordKind>>,
    unparsed_lines: Vec<String>,
}

impl TSplineProgram {
    /// Ordered recognized header declarations.
    #[must_use]
    pub fn headers(&self) -> &[TSplineProgramLine<TSplineHeaderKind>] {
        &self.headers
    }

    /// Ordered recognized topology, geometry, and constraint records.
    #[must_use]
    pub fn records(&self) -> &[TSplineProgramLine<TSplineRecordKind>] {
        &self.records
    }

    /// Non-comment lines outside the defined vocabulary.
    #[must_use]
    pub fn unparsed_lines(&self) -> &[String] {
        &self.unparsed_lines
    }
}

/// One tokenized T-spline program line of a single vocabulary bucket.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TSplineProgramLine<K> {
    kind: K,
    fields: Vec<String>,
}

impl<K: Copy> TSplineProgramLine<K> {
    /// Leading record or header token.
    #[must_use]
    pub fn kind(&self) -> K {
        self.kind
    }

    /// Ordered remaining fields without interpretation loss.
    #[must_use]
    pub fn fields(&self) -> &[String] {
        &self.fields
    }
}

impl TSplineProgram {
    /// Parse the defined line vocabulary while retaining every other line.
    #[must_use]
    pub fn parse(program: &str) -> Self {
        let mut parsed = Self {
            headers: Vec::new(),
            records: Vec::new(),
            unparsed_lines: Vec::new(),
        };
        for line in program.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split_whitespace();
            let Some(token) = fields.next() else { continue };
            if let Some(kind) = TSplineHeaderKind::from_token(token) {
                parsed.headers.push(TSplineProgramLine {
                    kind,
                    fields: fields.map(String::from).collect(),
                });
            } else if let Some(kind) = TSplineRecordKind::from_token(token) {
                parsed.records.push(TSplineProgramLine {
                    kind,
                    fields: fields.map(String::from).collect(),
                });
            } else {
                parsed.unparsed_lines.push(line.into());
            }
        }
        parsed
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetStation {
    /// Native spine parameter.
    pub knot: f64,
    /// Multiplicity of this parameter in the native knot vector.
    pub multiplicity: u32,
    /// Values and derivatives at this parameter.
    pub site: RollingBallJetSite,
}

const EPS_ROLLING_BALL_RADIUS: f64 = 1.0e-9;

/// Degree and finite clamped station data of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RollingBallJetReadWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetStations {
    degree: u32,
    stations: Vec<RollingBallJetStation>,
}

impl RollingBallJetStations {
    /// Admit clamped increasing knots and finite equal-radius station data.
    pub fn try_new(
        degree: u32,
        stations: Vec<RollingBallJetStation>,
    ) -> Result<Self, &'static str> {
        let maximum_multiplicity = degree.checked_add(1).filter(|_| degree != 0).ok_or(
            "rolling-ball jet degree must be positive with a representable end multiplicity",
        )?;
        if stations.len() < 2 {
            return Err("rolling-ball jet stations must contain at least two rows");
        }
        if stations[0].multiplicity != maximum_multiplicity
            || stations[stations.len() - 1].multiplicity != maximum_multiplicity
            || stations.iter().any(|station| {
                station.multiplicity == 0 || station.multiplicity > maximum_multiplicity
            })
        {
            return Err(
                "rolling-ball jet multiplicities must be in 1..=degree+1 with clamped ends",
            );
        }
        if stations.iter().any(|station| !station.knot.is_finite())
            || stations.windows(2).any(|pair| pair[0].knot >= pair[1].knot)
        {
            return Err("rolling-ball jet knots must be finite and strictly increasing");
        }
        for station in &stations {
            let site = &station.site;
            if [site.first_limit, site.second_limit, site.center]
                .iter()
                .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
                || !site.angle.is_finite()
            {
                return Err("rolling-ball jet site coordinates and angle must be finite");
            }
            for derivative in [&site.first_derivative, &site.second_derivative] {
                if [
                    derivative.first_limit,
                    derivative.second_limit,
                    derivative.center,
                ]
                .iter()
                .any(|vector| {
                    ![vector.x, vector.y, vector.z]
                        .into_iter()
                        .all(f64::is_finite)
                }) || !derivative.angle.is_finite()
                {
                    return Err("rolling-ball jet site derivatives must be finite");
                }
            }
            let first_radius = site.first_limit.distance(site.center);
            let second_radius = site.second_limit.distance(site.center);
            if !first_radius.is_finite()
                || first_radius <= 0.0
                || !second_radius.is_finite()
                || (first_radius - second_radius).abs()
                    > EPS_ROLLING_BALL_RADIUS * first_radius.max(second_radius).max(1.0)
            {
                return Err("rolling-ball jet site radii must be finite and agree within tolerance, with a positive first radius");
            }
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
    pub fn stations(&self) -> &[RollingBallJetStation] {
        &self.stations
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct RollingBallJetReadWire {
    degree: u32,
    knots: Vec<f64>,
    multiplicities: Vec<u32>,
    sites: Vec<RollingBallJetSite>,
}

impl TryFrom<RollingBallJetReadWire> for RollingBallJetStations {
    type Error = &'static str;

    fn try_from(wire: RollingBallJetReadWire) -> Result<Self, Self::Error> {
        if wire.knots.len() != wire.multiplicities.len() || wire.knots.len() != wire.sites.len() {
            return Err(
                "rolling-ball jet knots, multiplicities, and sites must have equal lengths",
            );
        }
        let stations = wire
            .knots
            .into_iter()
            .zip(wire.multiplicities)
            .zip(wire.sites)
            .map(|((knot, multiplicity), site)| RollingBallJetStation {
                knot,
                multiplicity,
                site,
            })
            .collect();
        Self::try_new(wire.degree, stations)
    }
}

#[derive(Serialize)]
struct RollingBallJetWriteWire<'a> {
    degree: u32,
    knots: Vec<f64>,
    multiplicities: Vec<u32>,
    sites: Vec<&'a RollingBallJetSite>,
}

impl Serialize for RollingBallJetStations {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollingBallJetWriteWire {
            degree: self.degree,
            knots: self.stations.iter().map(|station| station.knot).collect(),
            multiplicities: self
                .stations
                .iter()
                .map(|station| station.multiplicity)
                .collect(),
            sites: self.stations.iter().map(|station| &station.site).collect(),
        }
        .serialize(serializer)
    }
}

/// One aligned knot site of an exact rolling-ball surface jet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetSite {
    /// First limiting point at the knot.
    pub first_limit: Point3,
    /// Second limiting point at the knot.
    pub second_limit: Point3,
    /// Rolling-ball center at the knot.
    pub center: Point3,
    /// Signed opening angle at the knot, in radians.
    pub angle: f64,
    /// First parameter derivative of all four value channels.
    pub first_derivative: RollingBallJetDerivative,
    /// Second parameter derivative of all four value channels.
    pub second_derivative: RollingBallJetDerivative,
}

/// One derivative row for the four channels of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetDerivative {
    /// Derivative of the first limiting point.
    pub first_limit: Vector3,
    /// Derivative of the second limiting point.
    pub second_limit: Vector3,
    /// Derivative of the rolling-ball center.
    pub center: Vector3,
    /// Derivative of the signed opening angle.
    pub angle: f64,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RevisionSurfaceForm<F: Default = Vec<bool>> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Optional U/V bound fields following the support surface.
    #[serde(default)]
    pub support_bounds: [Option<f64>; 4],
    /// Optional parameter endpoints following the embedded reference curve.
    #[serde(default)]
    pub reference_endpoints: [Option<f64>; 2],
    /// Optional parameter endpoints following a second embedded curve, used
    /// by two-curve carriers such as `sum_spl_sur`.
    #[serde(default)]
    pub second_endpoints: [Option<f64>; 2],
    /// Carrier-specific boolean run preceding the shared tail.
    #[serde(default)]
    pub flags: F,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
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
    /// Native selector emitted for this cache form.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::SolvedCache { .. } => 0,
            Self::Parameterization(_) => 2,
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
    pub const fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::SolvedCache { fit_tolerance } => Some(fit_tolerance.get()),
            Self::Parameterization(_) => None,
        }
    }
}

/// Approximation state and its dependent fit contract for a variable blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableBlendCache {
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
        parameterization: RevisionSurfaceParameterization,
    },
}

impl VariableBlendCache {
    /// Native approximation-current flag.
    #[must_use]
    pub const fn shape_prefix(&self) -> i64 {
        match self {
            Self::Current { shape_prefix, .. } => shape_prefix.get(),
            Self::Stale {} => 0,
            Self::Parameterization { shape_prefix, .. } => *shape_prefix,
        }
    }

    /// Native tail selector.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::Current { .. } | Self::Stale {} => 0,
            Self::Parameterization { .. } => 2,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&RevisionSurfaceParameterization> {
        match self {
            Self::Parameterization {
                parameterization, ..
            } => Some(parameterization),
            _ => None,
        }
    }

    /// Active fit tolerance, absent for a stale or parameterized approximation.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::Current { fit_tolerance, .. } => Some(fit_tolerance.get()),
            _ => None,
        }
    }
}

/// Parameterization carried by tail-enum form `2` of the shared revision-gated
/// spline-surface tail. This form stores no approximation cache and no fit
/// tolerance; it stores the two parameter intervals followed by four enums, in
/// the order the fields appear below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RevisionSurfaceParameterization {
    /// U parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    #[serde(default)]
    pub u_interval: [Option<f64>; 2],
    /// V parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    #[serde(default)]
    pub v_interval: [Option<f64>; 2],
    /// U closure enum.
    pub u_closure: i64,
    /// V closure enum.
    pub v_closure: i64,
    /// U singularity enum.
    pub u_singularity: i64,
    /// V singularity enum.
    pub v_singularity: i64,
}

/// Subtype-specific tail of a native taper spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum TaperSurfaceKind {
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
        draft: Vector3,
    },
    /// Shadow taper with a pre-factored draft angle.
    Shadow {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
    },
    /// Ruled taper with a pre-factored angle and factor.
    Ruled {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
        /// Native ruled-taper factor.
        factor: f64,
    },
    /// Swept taper with a pre-factored draft angle.
    Swept {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
    },
}

/// One scalar row in native loft subdata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftSubdataRow {
    /// Leading ordered scalar pair.
    pub parameters: [f64; 2],
    /// Ordered per-column scalar pairs; empty for subdata type 211.
    pub columns: Vec<[f64; 2]>,
    /// Trailing scalar pair stored by the revision-gated row encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<[f64; 2]>,
}

/// Native loft constraint table with structurally consistent dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoftSubdata {
    /// Type 211 stores exactly one leading pair and no column pairs.
    Type211 {
        /// Native row/column header values. They do not count this form's payload.
        dimensions: [i64; 2],
        /// The sole leading scalar pair.
        row: [f64; 2],
    },
    /// All other table types store rows of one shared column width.
    Table(LoftSubdataTable),
}

/// Native loft table type discriminator other than the fixed type 211.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub struct TableTypeCode(i64);

/// Type 211 is the fixed single-row form, not a table type code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("loft subdata type 211 is the fixed single-row form, not a table")]
pub struct Type211IsNotATable;

impl TableTypeCode {
    /// Admit a table type code other than 211.
    pub const fn new(type_code: i64) -> Result<Self, Type211IsNotATable> {
        if type_code == 211 {
            return Err(Type211IsNotATable);
        }
        Ok(Self(type_code))
    }

    /// Native table type discriminator.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for TableTypeCode {
    type Error = Type211IsNotATable;

    fn try_from(type_code: i64) -> Result<Self, Self::Error> {
        Self::new(type_code)
    }
}

impl From<TableTypeCode> for i64 {
    fn from(type_code: TableTypeCode) -> Self {
        type_code.0
    }
}

/// Checked non-211 loft table payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LoftSubdataTableWire", into = "LoftSubdataTableWire")]
pub struct LoftSubdataTable {
    type_code: TableTypeCode,
    rows: Vec<LoftSubdataRow>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LoftSubdataTableWire {
    type_code: TableTypeCode,
    rows: Vec<LoftSubdataRow>,
}

/// The rows of a loft table do not share one column width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("loft subdata rows do not share one column width")]
pub struct RaggedLoftTable;

impl LoftSubdataTable {
    /// Admit a table whose rows share one column width.
    pub fn new(
        type_code: TableTypeCode,
        rows: Vec<LoftSubdataRow>,
    ) -> Result<Self, RaggedLoftTable> {
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

impl TryFrom<LoftSubdataTableWire> for LoftSubdataTable {
    type Error = RaggedLoftTable;

    fn try_from(wire: LoftSubdataTableWire) -> Result<Self, Self::Error> {
        Self::new(wire.type_code, wire.rows)
    }
}

impl From<LoftSubdataTable> for LoftSubdataTableWire {
    fn from(table: LoftSubdataTable) -> Self {
        Self {
            type_code: table.type_code,
            rows: table.rows,
        }
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
        let type_code = TableTypeCode::new(type_code).ok()?;
        LoftSubdataTable::new(type_code, rows).ok().map(Self::Table)
    }

    /// Native table type discriminator.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Type211 { .. } => 211,
            Self::Table(table) => table.type_code.get(),
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
    pub fn visit_rows(&self, mut visit: impl FnMut(&[f64; 2], &[[f64; 2]], Option<&[f64; 2]>)) {
        match self {
            Self::Type211 { row, .. } => visit(row, &[], None),
            Self::Table(table) => {
                for row in &table.rows {
                    visit(&row.parameters, &row.columns, row.extra.as_ref());
                }
            }
        }
    }

    /// Whether every leading and per-column scalar is finite.
    #[must_use]
    pub(crate) fn row_values_are_finite(&self) -> bool {
        let mut valid = true;
        self.visit_rows(|parameters, columns, _extra| {
            valid &= parameters.iter().all(|value| value.is_finite())
                && columns.iter().flatten().all(|value| value.is_finite());
        });
        valid
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
pub struct ClassicLoftProfileData {
    /// Required support surface.
    pub surface: SurfaceId,
    /// Nullable parameter curve on the support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
    /// First native constraint flag.
    pub first_flag: bool,
    /// ASM extension integer preceding the subdata.
    pub asm_extension: i64,
    /// Native constraint table.
    pub subdata: LoftSubdata,
    /// Optional direction selected by the second native flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Vector3>,
}

/// Type-selected fields of one loft profile member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoftMemberForm {
    /// Support-surface form. Legacy layouts can use type zero; revision-gated
    /// layouts select this form with a nonzero type code.
    Support {
        /// Native member type discriminator.
        type_code: i64,
        /// Constraint support surface, absent for the native `null_surface`
        /// sentinel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<SurfaceId>,
        /// Optional U/V bound fields following the support surface in the
        /// revision-gated encoding.
        #[serde(default)]
        support_bounds: [Option<f64>; 4],
        /// UV curve on the support, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// First native constraint flag.
        first_flag: bool,
        /// ASM extension integer when the stream version carries it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata,
        /// Optional direction selected by the second native flag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
    },
    /// Revision-gated type-zero form with two nullable UV curve slots.
    PcurvePair {
        /// First UV curve slot, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// Second UV curve slot, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secondary_pcurve: Option<PcurveGeometry>,
        /// ASM extension integer when the stream version carries it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata,
        /// Optional direction selected by the second native flag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
    },
}

impl LoftMemberForm {
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

    /// Return the first pcurve slot.
    #[must_use]
    pub fn pcurve(&self) -> Option<&PcurveGeometry> {
        match self {
            Self::Support { pcurve, .. } | Self::PcurvePair { pcurve, .. } => pcurve.as_ref(),
        }
    }

    /// Return the constraint subdata.
    #[must_use]
    pub fn subdata(&self) -> &LoftSubdata {
        match self {
            Self::Support { subdata, .. } | Self::PcurvePair { subdata, .. } => subdata,
        }
    }

    /// Return the optional direction selected by the second native flag.
    #[must_use]
    pub fn direction(&self) -> Option<&Vector3> {
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
pub struct LoftPathCurve {
    /// Referenced curve carrier.
    #[serde(rename = "curve")]
    pub id: CurveId,
    /// Optional parameter endpoints following the curve in a revision-gated encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<[Option<f64>; 2]>,
}

/// One curve member of a loft profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftProfileMember {
    /// Profile curve and its revision-gated parameter endpoints.
    pub profile: LoftPathCurve,
    /// Structurally selected surface-side constraint form.
    pub form: LoftMemberForm,
}

/// Native path data attached to one loft section entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftPath {
    /// Primary path curve and its optional endpoints, absent for `null_curve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<LoftPathCurve>,
    /// Ordered auxiliary BS3 curves.
    pub auxiliaries: Vec<CurveId>,
    /// Native path tail integer.
    pub flag: i64,
}

/// One parameterized entry in a native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftSectionEntry {
    /// Native section parameter.
    pub parameter: f64,
    /// Ordered profile members.
    pub profile: Vec<LoftProfileMember>,
    /// Native path data.
    pub path: LoftPath,
}

/// Revision-gated `loft_spl_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftRevisionForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Four booleans following the parameter intervals.
    #[serde(default)]
    pub flags: [bool; 4],
    /// Two integers preceding the shared tail.
    #[serde(default)]
    pub ints: [i64; 2],
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
}

/// Ordered native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LoftSection {
    /// Ordered entries in the section.
    pub entries: Vec<LoftSectionEntry>,
}

/// Token retained from the variable bridge preceding a loft solved cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum LoftBridgeToken {
    /// Native boolean token.
    Boolean(bool),
    /// Native integer token.
    Integer(i64),
    /// Native double token.
    Double(f64),
    /// Native string token.
    Text(String),
    /// Native enum token.
    Enum(i64),
}

/// Common carrier fields of one G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct G2BlendSide {
    /// Native side label.
    pub label: String,
    /// Primary support surface.
    pub surface: SurfaceId,
    /// Primary side curve.
    pub curve: CurveId,
    /// First and second ordered BS2 pcurves; each may be `nullbs`.
    pub pcurves: [Option<PcurveGeometry>; 2],
    /// Native side direction.
    pub direction: Vector3,
}

/// Singularity-specific payload of the first G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum G2BlendFirstShape {
    /// Full singularity with an optional BS3 support surface.
    Full {
        /// Exact BS3 support and fit tolerance, when serialized.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        support: Option<G2BlendFullSupport>,
    },
    /// Non-singular nine-scalar frame and tertiary pcurve.
    None {
        /// Ordered native frame scalars.
        coefficients: [f64; 9],
        /// Native fit tolerance.
        tolerance: FitTolerance,
        /// Optional intervening native token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extension: Option<LoftBridgeToken>,
        /// Tertiary BS2 pcurve, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct G2BlendConstruction {
    /// First side common fields.
    pub first: G2BlendSide,
    /// Native first-side singularity enum.
    pub singularity: i64,
    /// First-side singularity payload.
    pub first_shape: G2BlendFirstShape,
    /// Second side common fields.
    pub second: G2BlendSide,
    /// Exact second-side spline support.
    pub second_exact_surface: SurfaceId,
    /// Center or transition curve.
    pub center_curve: CurveId,
    /// Ordered center-curve scalars.
    pub center_parameters: [f64; 2],
    /// Native center tail integer.
    pub center_flag: i64,
    /// Native U and V intervals.
    pub parameter_ranges: [[f64; 2]; 2],
    /// Four ordered trailing scalars.
    pub trailing_parameters: [f64; 4],
    /// Three ordered ASM discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
}

/// A present rolling-ball support surface and its native UV bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSupportSurface<S = SurfaceId> {
    /// Support surface or embedded geometry.
    pub surface: S,
    /// Optional native U and V endpoints.
    pub parameter_ranges: [[Option<f64>; 2]; 2],
}

/// A present rolling-ball side curve and its native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSupportCurve<C = CurveId> {
    /// Side curve or embedded geometry.
    pub curve: C,
    /// Optional native parameter endpoints.
    pub parameter_range: [Option<f64>; 2],
}

/// The optional rolling-ball extension clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSideExtension<P = PcurveGeometry> {
    /// Native integer introducing the clause.
    pub value: i64,
    /// Tertiary BS2 pcurve, absent for `nullbs`.
    pub pcurve: Option<P>,
}

/// One complete native rolling-ball support side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(bound(deserialize = "S: Deserialize<'de>, C: Deserialize<'de>, P: Deserialize<'de>"))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSide<S = SurfaceId, C = CurveId, P = PcurveGeometry> {
    /// Geometry role selected by the support-side discriminator.
    pub support_kind: VariableBlendSupportKind,
    /// Primary support surface and bounds, absent for `null_surface`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<RollingBallSupportSurface<S>>,
    /// Side curve and bounds, absent for `null_curve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<RollingBallSupportCurve<C>>,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<P>,
    /// Native model-space side location.
    pub location: Point3,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_pcurve: Option<P>,
    /// Native extension integer and nullable tertiary pcurve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<RollingBallSideExtension<P>>,
}

/// Third support graph appended by `sss_blend_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallThirdSide {
    /// Native side label.
    pub label: String,
    /// Third support surface.
    pub surface: SurfaceId,
    /// Third side curve.
    pub curve: CurveId,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
    /// Native side vector.
    pub direction: Vector3,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_pcurve: Option<PcurveGeometry>,
    /// Native ASM integer following the secondary pcurve.
    pub extension: i64,
    /// ASM tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// Integer radius-selector value in a revision G2 blend.
///
/// The native `-1` value denotes the absence variant and is not a value of
/// this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct RevisionG2RadiusValue(i64);

impl RevisionG2RadiusValue {
    /// Construct an explicit selector value.
    #[must_use]
    pub const fn new(value: i64) -> Option<Self> {
        if value == -1 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Return the native integer value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for RevisionG2RadiusValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RevisionG2RadiusValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| {
            serde::de::Error::custom("revision G2 radius selector value cannot be -1")
        })
    }
}

/// Complete byte-backed rolling-ball or three-surface blend context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallConstruction {
    /// Native subtype definition-table index.
    pub definition_index: i64,
    /// Two ordered primary support sides.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored slice or center curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<f64>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [f64; 2],
    /// Optional-radius selector field.
    pub radius_selector: RollingBallRadiusSelector,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer preceding the trailing scalars.
    pub shape_prefix: i64,
    /// Two ordered trailing scalars.
    pub parameters: [f64; 2],
    /// Native long following the trailing scalars.
    pub tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// Native Boolean closing the shared tail.
    #[serde(default)]
    pub tail_flag: bool,
    /// Third side present only for `sss_blend_spl_sur`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third: Option<Box<RollingBallThirdSide>>,
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
pub struct VariableBlendInterpolationPoint {
    /// Law parameter.
    pub parameter: f64,
    /// Radius in document length units.
    pub radius: f64,
    /// Optional first and second derivative scalars.
    pub tangents: [Option<f64>; 2],
    /// Model-space control location.
    pub location: Point3,
    /// Control normal.
    pub normal: Vector3,
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
pub enum VariableBlendTerminal {
    /// Native double token.
    Double(f64),
    /// Native string token.
    Text(String),
}

/// Complete recursive native `getBlendValues` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VariableBlendValue {
    /// Native Boolean following the calibrated enum.
    pub modern_flag: bool,
    /// Native calibrated enum.
    pub calibrated: i64,
    /// Type-specific payload with its native sub-discriminator.
    pub payload: VariableBlendValuePayload,
}

/// Type-specific payload of a variable blend value.
///
/// Each arm carries the native sub-discriminator it admits: the edge-offset
/// arm is one of two codes, so an edge-offset value with any other
/// sub-discriminator has no spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableBlendValuePayload {
    /// Law-domain parameter range and two endpoint radii.
    TwoEnds {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Law-domain parameter range (lower, upper).
        parameters: [f64; 2],
        /// Endpoint radii in document length units.
        radii: [f64; 2],
    },
    /// Fixed-width branch: the parameter-range bounds and the chamfer width
    /// scalar, stored unscaled.
    FixedWidth {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Parameter-range lower and upper bounds.
        parameters: [f64; 2],
        /// Chamfer width.
        width: f64,
    },
    /// Edge-offset branch.
    EdgeOffset {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: EdgeOffsetDiscriminator,
        /// Ordered native scalar payload.
        scalars: [f64; 2],
        /// Ordered length payload in document units.
        lengths: [f64; 1],
    },
    /// Functional radius law carried by a BS2 pcurve.
    Functional {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: f64,
        /// Leading length in document units.
        radius: f64,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Numeric or symbolic terminal value.
        terminal: VariableBlendTerminal,
    },
    /// Constant law followed by a recursive chamfer value.
    Constant {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Ordered native scalars.
        parameters: [f64; 2],
        /// Radius in document length units.
        radius: f64,
        /// Native variable-chamfer enum.
        variable_chamfer: i64,
        /// Native chamfer-type enum.
        chamfer_type: i64,
        /// Recursively nested blend value.
        nested: Box<VariableBlendValue>,
    },
    /// Interpolated radius law.
    Interpolated {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: f64,
        /// Leading radius in document length units.
        radius: f64,
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
        points: Vec<VariableBlendInterpolationPoint>,
    },
}

impl VariableBlendValuePayload {
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
pub enum VariableBlendRadii {
    /// One radius law controls both support sides.
    Single {
        /// Shared radius law.
        value: VariableBlendValue,
    },
    /// Each support side has an independent radius law.
    Two {
        /// First support-side radius law.
        first: VariableBlendValue,
        /// Second support-side radius law.
        second: VariableBlendValue,
    },
}

impl VariableBlendRadii {
    /// First radius law in native order.
    #[must_use]
    pub const fn first(&self) -> &VariableBlendValue {
        match self {
            Self::Single { value } | Self::Two { first: value, .. } => value,
        }
    }

    /// Second radius law when both sides are controlled independently.
    #[must_use]
    pub const fn second(&self) -> Option<&VariableBlendValue> {
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
pub enum VariableBlendCrossSection {
    /// Circular section with no additional parameters.
    Circular {},
    /// Thumbweight-controlled section with two ordered shape parameters.
    Thumbweights {
        /// Ordered native shape parameters.
        parameters: [f64; 2],
    },
    /// Rounded chamfer with an optional independent rounding-radius law.
    RoundedChamfer {
        /// Rounding-radius law; absent when the clause stores `no_radius`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<Box<VariableBlendValue>>,
    },
    /// Curvature-continuous round with two ordered shape parameters.
    G2Round {
        /// Ordered native shape parameters.
        parameters: [f64; 2],
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VariableBlendConstruction {
    /// Native surface subtype selecting the variable-blend behavior class.
    #[serde(default)]
    pub subtype: VariableBlendSurfaceSubtype,
    /// Native serializer-revision integer following the subtype name.
    #[serde(alias = "definition_index")]
    pub revision: i64,
    /// Two ordered support-side graphs in the rolling-ball side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored slice curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<f64>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [f64; 2],
    /// Structurally selected radius-control payloads.
    pub radii: VariableBlendRadii,
    /// Cross-section clause following the complete radius-law sequence.
    /// Absence denotes an elided default circular section; an explicit
    /// circular clause remains distinct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_section: Option<VariableBlendCrossSection>,
    /// Support-side parameter interval `(T0, T1)`; both bounds present in
    /// every instance.
    pub u_range: [f64; 2],
    /// Second interval: a lower bound with an unbounded-above marker,
    /// encoded as `(T lo, F)` and decoding to `[Some(lo), None]`. The `F`
    /// upper-bound marker is an interval bound, not a standalone Boolean.
    #[serde(rename = "v_lower", default, skip_serializing_if = "Option::is_none")]
    pub v_lower: Option<f64>,
    /// Requested fit tolerance for the surface cache.
    pub shape_parameter: f64,
    /// Achieved fit tolerance for the surface cache, at or below
    /// `shape_parameter`, in document units.
    pub shape_length: f64,
    /// Non-negative integer immediately before the shared tail's enum.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: VariableBlendCache,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// Native Boolean following the discontinuity arrays.
    pub tail_flag: bool,
    /// Three ASM integers following the tail Boolean.
    pub tail_extensions: [i64; 3],
    /// Secondary curve and its bounds, absent for `null_curve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_curve: Option<RollingBallSupportCurve>,
    /// Blend convexity.
    pub convexity: VariableBlendConvexity,
    /// Solved-surface representation.
    pub render_mode: VariableBlendRenderMode,
    /// Native optional post-shape interval endpoints.
    pub post_range: [Option<f64>; 2],
    /// Native post-shape BS3 curve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_curve: Option<CurveId>,
    /// Native post-shape BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_pcurve: Option<PcurveGeometry>,
}

/// Complete native revision-gated `g2_blend_spl_sur` construction. The
/// revision layout stores the two support sides in the variable-blend side
/// layout and ends with the shared revision-gated surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RevisionG2BlendConstruction {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
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
    pub radius_selector: RollingBallRadiusSelector<RevisionG2RadiusValue>,
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

/// Complete native revision-gated `cl_loft_spl_sur` construction. The
/// revision layout is cache-first: the revision integer and shared
/// revision-gated surface tail precede the construction fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RevisionCompoundLoftConstruction {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
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

/// Trailing parameter bounds of a revision compound loft.
/// Both bounds select a trailing curve; all other forms contain no curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>")
)]
pub enum RevisionCompoundLoftTail<T> {
    /// Neither parameter bound is present.
    Unbounded {},
    /// Only the lower parameter bound is present.
    LowerBound {
        /// Lower parameter bound.
        lower: f64,
    },
    /// Only the upper parameter bound is present.
    UpperBound {
        /// Upper parameter bound.
        upper: f64,
    },
    /// Both parameter bounds and their selected curve.
    Curve {
        /// Ordered lower and upper parameter bounds.
        interval: [f64; 2],
        /// Curve selected by the complete parameter pair.
        curve: T,
    },
}

impl<T> RevisionCompoundLoftTail<T> {
    /// Optional bounds in native order.
    #[must_use]
    pub const fn interval(&self) -> [Option<f64>; 2] {
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

    /// Transform the curve payload while retaining its parameter bounds.
    #[must_use]
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> RevisionCompoundLoftTail<U> {
        match self {
            Self::Unbounded {} => RevisionCompoundLoftTail::Unbounded {},
            Self::LowerBound { lower } => RevisionCompoundLoftTail::LowerBound { lower },
            Self::UpperBound { upper } => RevisionCompoundLoftTail::UpperBound { upper },
            Self::Curve { interval, curve } => RevisionCompoundLoftTail::Curve {
                interval,
                curve: map(curve),
            },
        }
    }
}

/// One boundary record in a native vertex-blend patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VertexBlendBoundary {
    /// Native cross flag. The wire form is a logical, so the value is the
    /// tag itself and no payload follows.
    pub boundary_type: bool,
    /// Native magic direction. A unit direction or the zero vector, never a
    /// length, so it carries no unit scale.
    pub magic: Vector3,
    /// Native U-smoothing flag, a logical on the wire.
    pub u_smoothing: bool,
    /// Native V-smoothing flag, a logical on the wire.
    pub v_smoothing: bool,
    /// Native fullness scalar.
    pub fullness: f64,
    /// Structurally selected boundary geometry.
    pub geometry: VertexBlendBoundaryGeometry,
}

/// Twist payload selected by a vertex-blend circle form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VertexBlendTwists {
    /// Native form zero: no twist entries.
    None {},
    /// Native form one: one twist entry.
    One {
        /// The one twist entry.
        twist: Point3,
    },
    /// Native form three: two ordered twist entries.
    Two {
        /// The two ordered twist entries.
        twists: [Point3; 2],
    },
}

impl VertexBlendTwists {
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
    pub fn entries(&self) -> &[Point3] {
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
pub enum VertexBlendBoundaryGeometry {
    /// Curve boundary with a circle/ellipse/unknown twist form.
    Circle {
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<f64>; 2],
        /// Twist payload. Pre-revision layouts store model-space locations;
        /// the revision-gated layout stores unscaled twist vectors.
        twists: VertexBlendTwists,
        /// Two ordered curve parameters.
        parameters: [f64; 2],
        /// Native sense flag, a logical on the wire.
        sense: bool,
    },
    /// Degenerate boundary at a model-space location.
    Degenerate {
        /// Degenerate location.
        location: Point3,
        /// Two ordered boundary normals.
        normals: [Vector3; 2],
    },
    /// Surface pcurve boundary.
    Pcurve {
        /// Support surface.
        surface: SurfaceId,
        /// Optional U/V bound fields stored after the support by the
        /// revision-gated layout.
        #[serde(default)]
        support_bounds: [Option<f64>; 4],
        /// Native BS2 pcurve, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// Native sense flag, a logical on the wire.
        sense: bool,
        /// Parameter-space fit tolerance.
        fit_tolerance: FitTolerance,
    },
    /// Planar boundary described by a normal and curve.
    Plane {
        /// Plane normal.
        normal: Vector3,
        /// Two ordered plane parameters.
        parameters: [f64; 2],
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<f64>; 2],
    },
}

/// Complete native vertex-blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VertexBlendConstruction {
    /// Positive serializer-revision integer selecting the revision-gated
    /// layout; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    /// Ordered boundary records.
    pub boundaries: Vec<VertexBlendBoundary>,
    /// Native grid-size integer.
    pub grid_size: i64,
    /// Native model-space fit tolerance.
    pub fit_tolerance: FitTolerance,
}

/// One member of a compound-loft scale block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompoundLoftScaleMember {
    /// Native member integer.
    pub type_code: i64,
    /// Member curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData,
}

/// Complete `_readScaleClLoft` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompoundLoftScale {
    /// Ordered scale members.
    pub members: Vec<CompoundLoftScaleMember>,
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
pub enum CompoundLoftDirection {
    /// Inline direction vector. This form has no native selector.
    Vector {
        /// Stored direction.
        value: Vector3,
    },
    /// BS3 direction curve and its nonzero native selector.
    Curve {
        /// Stored curve.
        curve: CurveId,
        /// Exact nonzero native selector retained for byte-faithful export.
        selector: NonZeroI64,
    },
}

impl CompoundLoftDirection {
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
pub enum CompoundLoftTail {
    /// Native kind `6` tail.
    Six {
        /// Two leading flags.
        flags: [bool; 2],
        /// Required scale block.
        scale: Box<CompoundLoftScale>,
        /// Native integer following the scale.
        selector: i64,
        /// Stored direction.
        direction: Vector3,
        /// Native parameter interval.
        parameter_range: [f64; 2],
        /// BS3 tail curve.
        curve: CurveId,
    },
    /// Native kind `7` tail.
    Seven {
        /// First flag.
        first_flag: bool,
        /// First optional scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_scale: Option<Box<CompoundLoftScale>>,
        /// Second flag.
        second_flag: bool,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction.
        direction: Vector3,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
    /// Native kind `0` tail.
    Zero {
        /// Two leading flags.
        flags: [bool; 2],
        /// Vector or BS3 curve with its derived native selector.
        direction: CompoundLoftDirection,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
}

/// A bounded list of compound-loft scales.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<CompoundLoftScale>", into = "Vec<CompoundLoftScale>")]
pub struct CompoundLoftScales<const CAPACITY: usize>(Vec<CompoundLoftScale>);

#[cfg(feature = "schema")]
impl<const CAPACITY: usize> JsonSchema for CompoundLoftScales<CAPACITY> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("CompoundLoftScales_{CAPACITY}").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = Vec::<CompoundLoftScale>::json_schema(generator);
        schema.insert("maxItems".into(), CAPACITY.into());
        schema
    }
}

impl<const CAPACITY: usize> CompoundLoftScales<CAPACITY> {
    /// Admit a leading scale list within the native slot capacity.
    pub fn try_new(scales: Vec<CompoundLoftScale>) -> Result<Self, &'static str> {
        if scales.len() > CAPACITY {
            return Err("compound loft scales exceed slot capacity");
        }
        Ok(Self(scales))
    }

    /// Admit native optional slots whose present values form a leading prefix.
    pub fn try_from_slots(
        slots: impl IntoIterator<Item = Option<CompoundLoftScale>>,
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
    pub fn as_slice(&self) -> &[CompoundLoftScale] {
        &self.0
    }
}

impl<const CAPACITY: usize> TryFrom<Vec<CompoundLoftScale>> for CompoundLoftScales<CAPACITY> {
    type Error = &'static str;
    fn try_from(scales: Vec<CompoundLoftScale>) -> Result<Self, Self::Error> {
        Self::try_new(scales)
    }
}

impl<const CAPACITY: usize> From<CompoundLoftScales<CAPACITY>> for Vec<CompoundLoftScale> {
    fn from(scales: CompoundLoftScales<CAPACITY>) -> Self {
        scales.0
    }
}

/// Complete native compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompoundLoftConstruction {
    /// Present scales, up to the five native slots.
    pub scales: CompoundLoftScales<5>,
    /// Two flags before the tail kind.
    pub flags: [bool; 2],
    /// Kind-specific trailing graph.
    pub tail: CompoundLoftTail,
}

/// Initial solved-shape branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaledCompoundLoftShape {
    /// A solved NURBS cache follows the singularity enum.
    Full {},
    /// The cache is replaced by two intervals and two scalar arrays.
    None {
        /// Two ordered native intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Two ordered native scalar arrays.
        parameters: [Vec<f64>; 2],
    },
}

/// Structurally selected middle branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaledCompoundLoftBranch {
    /// Extended branch ending in a direction vector.
    ExtendedVector {
        /// Optional first scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_scale: Option<Box<CompoundLoftScale>>,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction vector.
        direction: Vector3,
    },
    /// Extended branch ending in a singularity and curve.
    ExtendedCurve {
        /// Optional scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<Box<CompoundLoftScale>>,
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
        direction: CompoundLoftDirection,
    },
}

/// Complete native scaled compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScaledCompoundLoftConstruction {
    /// Native leading singularity enum.
    pub singularity: i64,
    /// Singularity-selected solved-shape payload.
    pub shape: ScaledCompoundLoftShape,
    /// Six ordered discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Present leading scales within the three native slots.
    pub scales: CompoundLoftScales<3>,
    /// Two native flags preceding the selector.
    pub flags: [bool; 2],
    /// Native integer preceding the middle branch.
    pub selector: i64,
    /// Structurally selected middle branch.
    pub branch: ScaledCompoundLoftBranch,
    /// Two trailing branch flags.
    pub trailing_flags: [bool; 2],
    /// Native trailing kind integer.
    pub tail_kind: i64,
    /// Two native trailing vectors.
    pub tail_directions: [Vector3; 2],
    /// Native trailing singularity enum.
    pub tail_singularity: i64,
    /// Native trailing BS3 curve.
    pub tail_curve: CurveId,
}

/// A native law formula name that is neither empty nor the `null_law` sentinel.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct LawFormulaName(String);

impl LawFormulaName {
    /// Construct a named, non-sentinel law formula name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Option<Self> {
        let name = name.into();
        (!name.is_empty() && name != "null_law").then_some(Self(name))
    }

    /// Borrow the native formula name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for LawFormulaName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Self::new(name)
            .ok_or_else(|| serde::de::Error::custom("law formula name cannot be empty or null_law"))
    }
}

impl std::fmt::Display for LawFormulaName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One recursively framed native law formula.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LawFormula {
    /// Native `null_law` with no variables.
    Null {},
    /// Named formula and its ordered recursive variables.
    Named {
        /// Non-sentinel native formula name.
        name: LawFormulaName,
        /// Ordered recursive variables.
        variables: Vec<LawExpression>,
    },
}

impl LawFormula {
    /// Native formula name, including `null_law` for the null variant.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Null {} => "null_law",
            Self::Named { name, .. } => name.as_str(),
        }
    }

    /// Ordered recursive variables; empty for the null variant.
    #[must_use]
    pub fn variables(&self) -> &[LawExpression] {
        match self {
            Self::Null {} => &[],
            Self::Named { variables, .. } => variables,
        }
    }
}

/// Complete recursive construction stored by a native law spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LawSurfaceConstruction {
    /// Legacy U and V parameter intervals; absent from modern layouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_ranges: Option<[[f64; 2]; 2]>,
    /// Primary recursive surface law.
    pub primary: LawFormula,
    /// Ordered counted auxiliary laws referenced by the primary law.
    pub additional: Vec<LawFormula>,
    /// Standard surface-tail mode and its mode-specific fields.
    pub tail: LawSurfaceTail,
    /// Six ordered discontinuity arrays from the standard surface tail.
    pub discontinuities: [Vec<f64>; 6],
}

/// Mode-specific payload of a native law surface's standard surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LawSurfaceTail {
    /// Selector 0; the surface record carries a solved NURBS cache, whose fit
    /// contract this tail states.
    Full {
        /// Fit contract of the solved cache this tail requires.
        cache: LegacyCache,
    },
    /// Selector 1; compact parameter summaries replace the solved cache.
    Summary {
        /// Ordered U and V parameter summaries.
        parameters: [Vec<f64>; 2],
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
        parameter_ranges: [[f64; 2]; 2],
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
pub enum LawExpression {
    /// Zero-payload `null_law` sentinel.
    Null {},
    /// Serializer-preserved textual law expression.
    Text {
        /// Exact text stored in the native law slot.
        value: String,
    },
    /// Tagged integer constant.
    Integer {
        /// Stored integer value.
        value: i64,
    },
    /// Tagged double constant.
    Double {
        /// Stored scalar value.
        value: f64,
    },
    /// Tagged model-space point constant.
    Point {
        /// Stored point value.
        value: Point3,
    },
    /// Tagged direction-vector constant.
    Vector {
        /// Stored vector value.
        value: Vector3,
    },
    /// Inline transform-law payload.
    Transform {
        /// Thirteen ordered transform scalars.
        scalars: [f64; 13],
        /// Three ordered transform enums.
        enums: [i64; 3],
    },
    /// Vector-serialized transform-law payload: four ordered vectors, a scale,
    /// and three flags, in place of the thirteen-scalar/three-enum form.
    TransformVec {
        /// Four ordered transform vectors.
        vectors: [Vector3; 4],
        /// Trailing transform scale.
        scale: f64,
        /// Three ordered transform flags.
        flags: [bool; 3],
    },
    /// Curve-backed edge law.
    Edge {
        /// Embedded curve carrier and its optional revision-gated endpoints.
        #[serde(flatten)]
        curve: LoftPathCurve,
        /// Two native curve parameters.
        parameters: [f64; 2],
    },
    /// Spline-law payload.
    Spline {
        /// Native spline-law integer.
        native_id: i64,
        /// Ordered spline-law knots.
        knots: Vec<f64>,
        /// Ordered spline-law controls.
        controls: Vec<f64>,
        /// Native model-space point.
        point: Point3,
    },
    /// Algebraic operator and its recursively framed operands.
    Algebraic {
        /// Native operator token.
        operator: String,
        /// Ordered operands.
        operands: Vec<LawExpression>,
    },
}

/// One profile entry in the expanded skin layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SkinSurfaceProfile {
    /// Native profile type integer.
    pub type_code: i64,
    /// Profile curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData,
}

/// Structurally selected native skin payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkinSurfaceLayout {
    /// Expanded sequence of profile curves and loft constraints.
    Profiles {
        /// Ordered profile entries.
        profiles: Vec<SkinSurfaceProfile>,
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
        subdata: LoftSubdata,
        /// Integer after the subdata.
        first_tail: i64,
        /// Secondary curve.
        secondary_curve: CurveId,
        /// Final compact-layout integer.
        second_tail: i64,
    },
}

impl SkinSurfaceLayout {
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
pub struct SkinSurfaceConstruction {
    /// Native `SURF_BOOL` enum.
    pub surface_boolean: i64,
    /// Native `SURF_NORM` enum.
    pub surface_normal: i64,
    /// Native `SURF_DIR` enum.
    pub surface_direction: i64,
    /// Native leading count.
    pub count: i64,
    /// Native leading scalar.
    pub parameter: f64,
    /// Structurally selected skin payload.
    pub layout: SkinSurfaceLayout,
    /// Stored direction vector.
    pub direction: Vector3,
    /// Native scalar before the formula.
    pub trailing_parameter: f64,
    /// Recursive parametric law.
    pub formula: LawFormula,
    /// Trailing curve after the formula.
    pub parameter_curve: CurveId,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Complete native `net_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NetSurfaceConstruction {
    /// Two ordered loft-section graphs.
    pub sections: Box<[LoftSection; 2]>,
    /// Twelve ordered frame scalars.
    pub frame_parameters: [f64; 12],
    /// Native frame integer.
    pub flag: i64,
    /// Four ordered frame directions.
    pub directions: [Vector3; 4],
    /// Four ordered parameter laws.
    pub formulas: Box<[LawFormula; 4]>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Structurally selected native sweep payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SweepSurfaceLayout {
    /// Profile-first modern ASM sweep layout.
    ProfileFirst {
        /// Second native sweep enum.
        secondary_kind: i64,
        /// Five ordered frame directions.
        directions: [Vector3; 5],
        /// Native model-space frame origin.
        origin: Point3,
        /// Four ordered native frame scalars.
        parameters: [f64; 4],
        /// Three ordered parametric laws.
        formulas: Box<[LawFormula; 3]>,
    },
    /// Explicit sweep layout whose trajectory is controlled by a formula.
    ExplicitFormula {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Native formula-side boolean.
        formula_flag: bool,
        /// Parametric trajectory formula.
        formula: LawFormula,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
    /// Explicit sweep layout controlled by an auxiliary guide curve.
    ExplicitGuide {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Two guide-side booleans.
        guide_flags: [bool; 2],
        /// Auxiliary guide curve.
        guide_curve: CurveId,
        /// Guide parameter interval.
        guide_range: [f64; 2],
        /// Two native guide integers.
        guide_modes: [i64; 2],
        /// Six ordered guide scalars.
        guide_parameters: [f64; 6],
        /// Three trailing guide booleans.
        trailing_flags: [bool; 3],
    },
    /// Explicit sweep layout controlled by a support surface.
    ExplicitSurface {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Native singularity enum.
        singularity: i64,
        /// Support surface controlling the sweep.
        support_surface: SurfaceId,
        /// Optional auxiliary curve.
        auxiliary_curve: Option<CurveId>,
        /// Native support-side boolean.
        support_flag: bool,
        /// Legacy pre-219 trailing boolean when present.
        legacy_flag: Option<bool>,
    },
    /// Explicit-prefix sweep layout controlled by recursive laws.
    LawDriven {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Leading recursive sweep law.
        first_law: Box<LawExpression>,
        /// Native integer after the leading law.
        first_mode: i64,
        /// First law parameter interval.
        first_range: [f64; 2],
        /// Native law direction.
        law_direction: Vector3,
        /// Native path integer.
        path_mode: i64,
        /// Native path boolean.
        path_flag: bool,
        /// Path parameter interval.
        path_range: [f64; 2],
        /// Native path scalar.
        path_parameter: f64,
        /// Native second-law boolean.
        second_law_flag: bool,
        /// Trailing recursive sweep law.
        second_law: Box<LawExpression>,
        /// Native integer before the formula.
        formula_mode: i64,
        /// Parametric trajectory formula.
        formula: LawFormula,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
}

/// Revision-gated `sweep_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepRevisionForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Boolean replacing the pre-revision primary enum.
    pub primary_flag: bool,
    /// Optional parameter endpoints following the embedded profile curve.
    #[serde(default)]
    pub profile_endpoints: [Option<f64>; 2],
    /// Optional parameter endpoints following the embedded path curve.
    #[serde(default)]
    pub path_endpoints: [Option<f64>; 2],
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
}

/// Complete native `sweep_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepSurfaceConstruction {
    /// Leading native sweep enum.
    pub primary_kind: i64,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    pub cache: CacheContract<SweepRevisionForm>,
    /// Structurally selected sweep layout.
    pub layout: SweepSurfaceLayout,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
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
        signed_radius: f64,
    },
    /// Radius varying linearly from `start` to `end` along the spine.
    Linear {
        /// Signed radius at the spine start, in document length units.
        start: f64,
        /// Signed radius at the spine end, in document length units.
        end: f64,
    },
    /// Radius varying along the spine per an explicit law curve.
    Law {
        /// Curve whose parameterization gives the signed radius along the spine.
        curve: NurbsCurve,
    },
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

    /// Return the mapped pcurve endpoints as an array.
    #[must_use]
    pub const fn parameter_range_array(&self) -> Option<[f64; 2]> {
        match self.parameter_range {
            Some(range) => Some(range.endpoints()),
            None => None,
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

/// One paired surface and parameter-space curve in an intcurve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IntcurveSupportSide {
    /// Supporting surface, absent for the native `null_surface` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceId>,
    /// UV curve on `surface`, absent for the native `nullbs` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<SupportPcurve>,
}

impl IntcurveSupportSide {
    /// Return the mapped pcurve endpoints as an array.
    #[must_use]
    pub fn pcurve_parameter_range(&self) -> Option<[f64; 2]> {
        self.pcurve
            .as_ref()
            .and_then(SupportPcurve::parameter_range_array)
    }

    /// Map one solved-curve parameter into this side's pcurve parameter.
    ///
    /// Returns `None` when this side has no pcurve, an explicit map has an invalid
    /// solved interval, or the mapped parameter cannot be represented as finite.
    #[must_use]
    pub fn pcurve_parameter(
        &self,
        solved_parameter_range: [f64; 2],
        parameter: f64,
    ) -> Option<f64> {
        if !parameter.is_finite() {
            return None;
        }
        let pcurve_range = self
            .pcurve
            .as_ref()?
            .parameter_range
            .map(DirectedParameterRange::endpoints);
        let Some(pcurve_range) = pcurve_range else {
            return Some(parameter);
        };
        let solved_span = solved_parameter_range[1] - solved_parameter_range[0];
        if solved_span == 0.0
            || solved_parameter_range
                .iter()
                .any(|value| !value.is_finite())
        {
            return None;
        }
        if parameter == solved_parameter_range[0] {
            return Some(pcurve_range[0]);
        }
        if parameter == solved_parameter_range[1] {
            return Some(pcurve_range[1]);
        }
        let offset = parameter - solved_parameter_range[0];
        let fraction = if solved_span.is_finite() && offset.is_finite() {
            offset / solved_span
        } else {
            // Halving before subtraction retains finite opposite-sign endpoints.
            (parameter * 0.5 - solved_parameter_range[0] * 0.5)
                / (solved_parameter_range[1] * 0.5 - solved_parameter_range[0] * 0.5)
        };
        if !fraction.is_finite() {
            return None;
        }
        let mapped = pcurve_range[0] + fraction * (pcurve_range[1] - pcurve_range[0]);
        if mapped.is_finite() {
            Some(mapped)
        } else {
            let mapped = (1.0 - fraction) * pcurve_range[0] + fraction * pcurve_range[1];
            mapped.is_finite().then_some(mapped)
        }
    }
}

/// Version-stamp prefix and unbounded interval carried by the stamped
/// `law_int_cur` serializer form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LawCurveVersionForm {
    /// Serializer version stamp emitted after the subtype name.
    pub stamp: i64,
    /// Native enum following the version stamp.
    pub post_enum: i64,
    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    pub parameter_range: [Option<f64>; 2],
}

/// Shared support surfaces, UV curves, interval, and discontinuity arrays of a native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "IntcurveSupportContextWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct IntcurveSupportContext {
    sides: [IntcurveSupportSide; 2],
    parameter_range: [f64; 2],
    discontinuities: [Vec<f64>; 3],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct IntcurveSupportContextWire {
    sides: [IntcurveSupportSide; 2],
    parameter_range: [f64; 2],
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
        if !parameter_range.iter().all(|value| value.is_finite())
            || parameter_range[0] > parameter_range[1]
        {
            return Err("support context parameter_range must be finite and ordered");
        }
        if parameter_range[0] == parameter_range[1]
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
        if !discontinuities
            .iter()
            .flatten()
            .all(|value| value.is_finite())
        {
            return Err("support context discontinuities must be finite");
        }
        Ok(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }

    /// Edit the context transactionally and retain its previous value on rejection.
    pub fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut [IntcurveSupportSide; 2], &mut [f64; 2], &mut [Vec<f64>; 3]) -> R,
    ) -> Result<R, &'static str> {
        let mut candidate = self.clone();
        let result = edit(
            &mut candidate.sides,
            &mut candidate.parameter_range,
            &mut candidate.discontinuities,
        );
        *self = Self::try_new(
            candidate.sides,
            candidate.parameter_range,
            candidate.discontinuities,
        )?;
        Ok(result)
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
    pub const fn parameter_range(&self) -> [f64; 2] {
        self.parameter_range
    }

    /// Return the ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<f64>; 3] {
        &self.discontinuities
    }
}

/// Finite endpoint witnesses and distinct supports of a tolerant intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionConstructionWire")]
pub struct TolerantIntersectionConstruction {
    supports: [SurfaceId; 2],
    endpoints: [Point3; 2],
    tolerance: NonNegativeScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TolerantIntersectionConstructionWire {
    supports: [SurfaceId; 2],
    endpoints: [Point3; 2],
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
        if !endpoints
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        {
            return Err("tolerant intersection endpoints must be finite");
        }
        let tolerance = NonNegativeScalar::new(tolerance)
            .ok_or("tolerant intersection tolerance must be finite and non-negative")?;
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
    pub const fn endpoints(&self) -> &[Point3; 2] {
        &self.endpoints
    }

    /// Return the tolerance.
    #[must_use]
    pub const fn tolerance(&self) -> f64 {
        self.tolerance.get()
    }
}

/// Complete neutral parameterization of one topology-bounded intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionParameterizationWire")]
pub struct TolerantIntersectionParameterization {
    /// Coincident support charts in support order.
    pub pcurves: [PcurveGeometry; 2],
    parameter_range: [f64; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TolerantIntersectionParameterizationWire {
    pcurves: [PcurveGeometry; 2],
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
        if !parameter_range.iter().all(|value| value.is_finite())
            || parameter_range[0] >= parameter_range[1]
        {
            return Err(
                "tolerant intersection parameter_range must be finite and strictly increasing",
            );
        }
        Ok(Self {
            pcurves,
            parameter_range,
        })
    }

    /// Common finite solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> [f64; 2] {
        self.parameter_range
    }
}

/// Cache-first shared-context fields absent from the context-first layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CacheFirstCurveForm {
    /// Positive serializer-revision integer selecting the cache-first layout.
    pub revision: i64,
    /// Approximation-cache form selected by the shared context enum.
    pub cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    pub support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub solved_range: [Option<f64>; 2],
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
pub enum SpringSupport {
    /// Resolved support surface.
    Surface(SurfaceId),
    /// Native U/V ranges stored in place of `null_surface`.
    Ranges([[f64; 2]; 2]),
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
pub enum SpringPcurve {
    /// Resolved parameter-space curve.
    Pcurve(PcurveGeometry),
    /// Native interval stored in place of `nullbs`.
    Range([f64; 2]),
}

/// Mutually exclusive spring construction layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
pub enum SpringLayout {
    /// Support-first layout with inline null-carrier replacement ranges.
    ContextFirst {
        /// Two ordered support slots.
        supports: [SpringSupport; 2],
        /// First pcurve or its null replacement range.
        first_pcurve: SpringPcurve,
        /// Nullable second pcurve slot.
        second_pcurve: Option<PcurveGeometry>,
        /// Native solved-curve parameter interval.
        parameter_range: [f64; 2],
        /// Three ordered discontinuity arrays.
        discontinuities: [Vec<f64>; 3],
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Solved-cache fit contract this layout states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Cache-first layout, which carries no inline replacement ranges or
    /// context-first discontinuity flag.
    CacheFirst {
        /// Shared support context following the solved cache.
        context: IntcurveSupportContext,
        /// Cache-first serializer fields.
        form: CacheFirstCurveForm,
    },
}

impl SpringLayout {
    /// Return the support context, deriving it for the context-first layout.
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
            } => Ok(std::borrow::Cow::Owned(IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: match &supports[0] {
                            SpringSupport::Surface(surface) => Some(surface.clone()),
                            SpringSupport::Ranges(_) => None,
                        },
                        pcurve: match first_pcurve {
                            SpringPcurve::Pcurve(pcurve) => {
                                Some(SupportPcurve::new(pcurve.clone(), None))
                            }
                            SpringPcurve::Range(_) => None,
                        },
                    },
                    IntcurveSupportSide {
                        surface: match &supports[1] {
                            SpringSupport::Surface(surface) => Some(surface.clone()),
                            SpringSupport::Ranges(_) => None,
                        },
                        pcurve: second_pcurve
                            .clone()
                            .map(|pcurve| SupportPcurve::new(pcurve, None)),
                    },
                ],
                *parameter_range,
                discontinuities.clone(),
            )?)),
        }
    }

    fn cache_first(&self) -> Option<&CacheFirstCurveForm> {
        match self {
            Self::CacheFirst { form, .. } => Some(form),
            Self::ContextFirst { .. } => None,
        }
    }

    fn cache_first_mut(&mut self) -> Option<&mut CacheFirstCurveForm> {
        match self {
            Self::CacheFirst { form, .. } => Some(form),
            Self::ContextFirst { .. } => None,
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
pub struct CacheFirstCurveParameterization {
    /// Curve interval, an ordered `[lo, hi]` pair of optional bounds. `None` is
    /// a false bound-presence flag.
    #[serde(default)]
    pub interval: [Option<f64>; 2],
    /// Closed-form enum following the interval.
    pub closed_form: i64,
}

/// Family-independent tail fields carried by a cache-first surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SurfaceCurveTail {
    /// Native integer following the discontinuity arrays.
    pub extension: i64,
    /// Positive serializer-revision integer opening the cache-first layout.
    #[serde(default)]
    pub revision: i64,
    /// Approximation-cache form selected by the shared context enum.
    pub cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    pub support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub solved_range: [Option<f64>; 2],
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_flag: Option<bool>,
}

/// Mutually exclusive tail forms of a native projected intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionTail {
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
        parameter_range: [f64; 2],
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Curve constrained to a support surface.
    SurfaceConstrained {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Parametric curve on a support surface.
    Parametric {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields with the parametric-only second flag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail: Option<SurfaceCurveCacheFirst<ParametricSurfaceCurveFlags>>,
    },
    /// Skin curve on a support surface.
    Skin {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

    /// Mutably borrow the shared support context.
    #[must_use]
    pub fn context_mut(&mut self) -> &mut IntcurveSupportContext {
        match self {
            Self::Blend { context, .. }
            | Self::SurfaceConstrained { context, .. }
            | Self::Parametric { context, .. }
            | Self::Skin { context, .. } => context,
        }
    }

    fn revision_cache(&self) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_ref().map(|first| &first.form.cache),
            Self::Parametric { tail, .. } => tail.as_ref().map(|first| &first.form.cache),
        }
    }

    fn revision_cache_mut(
        &mut self,
    ) -> Option<&mut RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_mut().map(|first| &mut first.form.cache),
            Self::Parametric { tail, .. } => tail.as_mut().map(|first| &mut first.form.cache),
        }
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
pub enum DeformableCurveData {
    /// Mode 8 vector field followed by ordered scalar pairs.
    VectorField {
        /// Four ordered native vectors.
        vectors: [Vector3; 4],
        /// Ordered pairs from the mode-8 scalar table.
        parameter_pairs: Vec<[f64; 2]>,
    },
    /// Mode 3 fixed deformation payload.
    Mode3 {
        /// Four vectors at the start of the payload.
        leading_vectors: [Vector3; 4],
        /// Scalar following the leading vectors.
        leading_parameter: f64,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position following the leading flags.
        trailing_point: Point3,
        /// Two vectors following the position.
        trailing_vectors: [Vector3; 2],
        /// Scalar following the trailing frame.
        frame_parameter: f64,
        /// Two flags following the frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalars following the frame flags.
        parameters: [f64; 3],
        /// Five flags following the ordered scalars.
        trailing_flags: [bool; 5],
        /// Final scalar before the trailing integer.
        trailing_parameter: f64,
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
pub enum OffsetSide {
    /// Unit plane normal defining the positive offset side.
    PlaneNormal {
        /// Unit plane normal.
        normal: Vector3,
    },
    /// Explicit offset direction, optionally constrained to a support surface.
    Direction {
        /// Nonzero offset direction.
        direction: Vector3,
        /// Support surface within which the offset is measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        support: Option<SurfaceId>,
    },
}

/// Parameter interval and optional variable-distance law of a curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveOffsetRange {
    /// Constant-distance offset over a retained source interval.
    Uniform {
        /// Parameter interval on the source curve.
        parameter_range: [f64; 2],
    },
    /// Variable-distance offset over the interval used by its law.
    Variable {
        /// Parameter interval on the source curve.
        parameter_range: [f64; 2],
        /// Variable signed-distance law.
        distance_law: CurveOffsetDistanceLaw,
    },
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
        primary: LawFormula,
        /// Counted additional recursive law formulas.
        additional: Vec<LawFormula>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    Law {
        context: IntcurveSupportContext,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<LawCurveVersionForm>,
        extension: i64,
        primary: LawFormula,
        additional: Vec<LawFormula>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    Compound(CompoundCurveConstruction),
    Helix(HelixCurveConstruction),
    Intersection {
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    TolerantIntersection {
        construction: TolerantIntersectionConstruction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameterization: Option<TolerantIntersectionParameterization>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blend_surface: Option<SurfaceId>,
    },
    Unknown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
    fn revision_cache(&self) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization>> {
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

    fn revision_cache_mut(
        &mut self,
    ) -> Option<&mut RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::SurfaceCurve { family } => family.revision_cache_mut(),
            Self::SurfaceOffset(payload) => payload.revision_cache_mut(),
            Self::Spring(payload) => payload.revision_cache_mut(),
            Self::Deformable(payload) => Some(payload.revision_cache_mut()),
            _ => None,
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
    pub fn new(
        id: ProceduralCurveId,
        definition: ProceduralCurveDefinition,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self { id, definition })
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
    pub fn cache_fit_tolerance(&self) -> Option<f64> {
        self.definition.cache_fit_tolerance().map(FitTolerance::get)
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

    /// Raise the fit tolerance of an existing solved cache.
    pub fn raise_cache_fit_tolerance(&mut self, value: FitTolerance) {
        self.definition.raise_cache_fit_tolerance(value);
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
    pub fn scale_cache_fit_tolerance(&mut self, scale: f64) -> Result<(), CacheContractError> {
        if let Some(value) = self.cache_fit_tolerance() {
            self.set_cache_fit_tolerance(Some(value * scale))?;
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
    pub(crate) id: ProceduralSurfaceId,
    /// Carrier surface this construction produces.
    pub(crate) surface: SurfaceId,
    /// Neutral construction definition.
    pub(crate) definition: ProceduralSurfaceDefinition,
    /// Four optional U/V parameter bounds following the record's subtype scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) record_bounds: Option<[Option<f64>; 4]>,
}

/// One procedural-curve row: the construction and the carrier it produces.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct ProceduralCurveRow {
    /// Stable construction identity.
    pub(crate) id: ProceduralCurveId,
    /// Carrier curve this construction produces.
    pub(crate) curve: CurveId,
    /// Neutral construction definition.
    pub(crate) definition: ProceduralCurveDefinition,
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
            ProceduralSurface {
                id: self.id,
                definition: self.definition,
                record_bounds: self.record_bounds,
            },
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
pub enum CurveOffsetDistanceLaw {
    /// Linear interpolation between two distance controls.
    Linear {
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Ordered signed distances in document length units.
        distances: [f64; 2],
        /// Ordered arc-length or neutral carrier-parameter controls.
        control_range: [f64; 2],
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
        function_parameter_offset: f64,
        /// Function-parameter change per neutral source parameter or length unit.
        function_parameter_scale: f64,
    },
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
    pub(crate) const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
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
    pub(crate) const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
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
    pub(crate) fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}
