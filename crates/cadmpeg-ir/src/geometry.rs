// SPDX-License-Identifier: Apache-2.0
//! Geometry carriers: analytic surfaces, analytic curves, NURBS, and pcurves.
//!
//! Carriers are stored in their own arenas and referenced by id from the
//! topology graph (a face references a [`Surface`], an edge a [`Curve`], a
//! coedge a [`pcurve::Pcurve`]). One carrier may therefore support several topological
//! entities.

use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use crate::math::Vector3;
use crate::provenance::SourceObjectAssociation;
use crate::scalar::FiniteReal;
use crate::transform::Transform;
use analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
    LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_derive::DeserializeName;
use intersection_curves::IntcurveSupportContext;
use legacy_cache::{CacheContractError, FitTolerance, LegacyCache};
use nurbs::{NurbsCurve, NurbsSurface};
use pcurve::PcurveGeometry;
use procedural_curves::{DirectedParameterRange, ProceduralCurveDefinition};
use procedural_surfaces::ProceduralSurfaceDefinition;
use revision_forms::RevisionSurfaceForm;
use sampled::{PolygonalSurface, PolylineCurve};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod blends;
pub mod curve_offsets;
pub mod deformable_surfaces;
pub mod helix;
pub mod intersection_curves;
pub mod laws;
pub mod legacy_cache;
pub mod lofts;
pub mod procedural_curves;
pub mod procedural_surfaces;
pub mod revision_forms;
pub mod skin_surfaces;
pub mod surface_curves;
pub mod sweep_surfaces;
pub mod tsplines;
pub mod variable_blends;

pub mod analytic;
pub mod nurbs;
pub mod pcurve;
pub mod sampled;
pub mod scaling;

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
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
        form: Box<RevisionSurfaceForm<[bool; 4], R>>,
    },
}

/// Analytic, NURBS, or opaque surface geometry established without a
/// construction.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
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

    /// `"plane"`, `"cylinder"`, or `"cone"` where this carrier is one through
    /// every placement: a surface no face covers whole, so a face on it states
    /// a loop (part 10, Topology and model records).
    #[must_use]
    pub fn plane_cylinder_or_cone(&self) -> Option<&'static str> {
        match self {
            Self::Plane(_) => Some("plane"),
            Self::Cylinder(_) => Some("cylinder"),
            Self::Cone(_) => Some("cone"),
            Self::Transformed(placed) => placed.basis().plane_cylinder_or_cone(),
            Self::Sphere(_)
            | Self::Torus(_)
            | Self::Nurbs(_)
            | Self::Polygonal(_)
            | Self::Unknown { .. } => None,
        }
    }
}

/// Exact affine placement of an inline basis surface.
///
/// `try_new` is the only constructor and refuses a chain deeper than
/// [`MAX_GEOMETRY_NESTING`], so no [`SolvedSurfaceGeometry`] value nests past
/// the bound however it was built or read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
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
        cache: Option<SolvedSurfaceGeometry>,
    },
    /// A carrier whose shape is established without a construction.
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

    /// States this carrier in the form that holds only what was read
    /// (§3.3). `construction` states whether its procedural construction is
    /// dropped, and `held` whether the geometry it holds besides that
    /// construction needs a dropped record. A procedural carrier whose
    /// construction stays loses its cache where the cache needs a dropped
    /// record. One whose construction is dropped is its cache where the cache
    /// needs none, and `Unknown` otherwise. A solved carrier that needs a
    /// dropped record is `Unknown`.
    pub(crate) fn release(&mut self, construction: bool, held: bool) {
        let unknown = || Self::Solved(SolvedSurfaceGeometry::Unknown { record: None });
        match self {
            Self::Procedural { cache, .. } if !construction => {
                if held {
                    *cache = None;
                }
            }
            Self::Procedural { cache, .. } => {
                *self = cache
                    .take()
                    .filter(|_| !held)
                    .map_or_else(unknown, Self::Solved);
            }
            Self::Solved(_) if held => *self = unknown(),
            Self::Solved(_) => {}
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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
    /// The source record this surface projects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The analytic or free-form shape of a 3D curve carrier established without a
/// construction.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_self_intersect"
        )]
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

    /// The canonical interval of one traversal of this carrier as a closed
    /// curve: one revolution `[0, 2π]` of a circle or an ellipse, and the
    /// effective knot domain of a clamped or periodic NURBS curve. A placement
    /// keeps the interval of its basis. The carrier closes over the interval
    /// only where both ends evaluate to one point, so a closed edge with no
    /// stored range traverses this interval when its vertex lies at both ends.
    #[must_use]
    pub fn closed_traversal_range(&self) -> Option<crate::topology::IncreasingParameterInterval> {
        match self {
            Self::Circle(_) | Self::Ellipse(_) => {
                crate::topology::IncreasingParameterInterval::new([0.0, std::f64::consts::TAU])
            }
            Self::Nurbs(nurbs) => crate::eval::nurbs_curve_parameter_domain(nurbs),
            Self::Transformed(placed) => placed.basis().closed_traversal_range(),
            Self::Line(_)
            | Self::Parabola(_)
            | Self::Hyperbola(_)
            | Self::Degenerate(_)
            | Self::Composite { .. }
            | Self::Polyline(_)
            | Self::Unknown { .. } => None,
        }
    }
}

/// Exact affine placement of an inline basis curve.
///
/// `try_new` is the only constructor and refuses a chain deeper than
/// [`MAX_GEOMETRY_NESTING`], so no [`SolvedCurveGeometry`] value nests past the
/// bound however it was built or read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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

/// A curve carrier held outside a `Topology`: a leaf carrier, or an inline
/// placement of one. It names no record.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum InlineCurve {
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
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
    /// Source-native polyline with an explicit chordal error bound.
    Polyline(PolylineCurve),
    /// Exact affine placement of an inline basis curve.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<InlineCurve>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
}

/// A surface carrier held outside a `Topology`: a leaf carrier, or an inline
/// placement of one. It names no record.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum InlineSurface {
    /// Infinite plane.
    Plane(PlaneSurface),
    /// Right circular cylinder.
    Cylinder(CylinderSurface),
    /// Right elliptical cone.
    Cone(ConeSurface),
    /// Sphere.
    Sphere(SphereSurface),
    /// Torus.
    Torus(TorusSurface),
    /// Free-form NURBS surface.
    Nurbs(NurbsSurface),
    /// Source-native polygonal surface with an explicit chordal error bound.
    Polygonal(PolygonalSurface),
    /// Exact affine placement of an inline basis surface.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<InlineSurface>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
}

/// The analytic, free-form, procedural, or opaque shape of a 3D curve carrier.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
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
        cache: Option<SolvedCurveGeometry>,
    },
    /// A carrier whose shape is established without a construction.
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

    /// States this carrier in the form that holds only what was read
    /// (§3.3). `construction` states whether its procedural construction is
    /// dropped, and `held` whether the geometry it holds besides that
    /// construction needs a dropped record. A procedural carrier whose
    /// construction stays loses its cache where the cache needs a dropped
    /// record. One whose construction is dropped is its cache where the cache
    /// needs none, and `Unknown` otherwise. A solved carrier that needs a
    /// dropped record is `Unknown`.
    pub(crate) fn release(&mut self, construction: bool, held: bool) {
        let unknown = || Self::Solved(SolvedCurveGeometry::Unknown { record: None });
        match self {
            Self::Procedural { cache, .. } if !construction => {
                if held {
                    *cache = None;
                }
            }
            Self::Procedural { cache, .. } => {
                *self = cache
                    .take()
                    .filter(|_| !held)
                    .map_or_else(unknown, Self::Solved);
            }
            Self::Solved(_) if held => *self = unknown(),
            Self::Solved(_) => {}
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
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
    } else if let Some(unit) = crate::features::geometry::FiniteVector3::new(axis)
        .and_then(crate::features::geometry::FiniteVector3::unit_nonzero)
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Curve {
    /// Arena id.
    pub id: CurveId,
    /// Curve shape.
    pub geometry: CurveGeometry,
    /// Finite source interval of the carrier, independent of topology trims.
    /// The underlying mathematical curve can still be used outside this interval.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier_parameter_range"
    )]
    pub parameter_range: Option<crate::topology::IncreasingParameterInterval>,
    /// Native source-object identity and effective display metadata.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_object"
    )]
    pub source_object: Option<SourceObjectAssociation>,
    /// The source record this curve projects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Four optional finite parameter bounds retained from one native surface
/// record.
///
/// The positions are the native record's fields. Their meaning is supplied
/// by the owning subtype, so a partial quartet remains valid. `None` in one
/// position means that field was absent in the source record.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_procedural_surface_record_bounds"
    )]
    record_bounds: Option<RecordBounds>,
    /// The source record this construction projects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
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
            native_ref: None,
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
        self.definition.scale_cache_fit_tolerance(scale)
    }
}

/// A neutral curve construction linked to its solved carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProceduralCurve {
    /// Stable construction identity.
    pub id: ProceduralCurveId,
    /// Neutral construction definition, which states its own cache contract.
    definition: ProceduralCurveDefinition,
    /// The source record this construction projects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl ProceduralCurve {
    /// Build a procedural curve from its construction definition.
    pub fn new(id: ProceduralCurveId, definition: ProceduralCurveDefinition) -> Self {
        Self {
            id,
            definition,
            native_ref: None,
        }
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
        self.definition.scale_cache_fit_tolerance(scale)
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
    /// The source record this construction projects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    native_ref: Option<String>,
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
    /// The source record this construction projects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    native_ref: Option<String>,
}

impl ProceduralSurfaceRow {
    /// The construction's identity.
    pub(crate) fn id_str(&self) -> &str {
        self.id.as_str()
    }

    /// The row a construction and its carrier state.
    pub(crate) fn new(surface: SurfaceId, procedural: &ProceduralSurface) -> Self {
        Self {
            id: procedural.id.clone(),
            surface,
            definition: procedural.definition.clone(),
            record_bounds: procedural.record_bounds,
            native_ref: procedural.native_ref.clone(),
        }
    }

    /// The carrier and the construction this row states.
    pub(crate) fn into_parts(self) -> (SurfaceId, ProceduralSurface) {
        (
            self.surface,
            ProceduralSurface {
                native_ref: self.native_ref,
                ..ProceduralSurface::new(self.id, self.definition, self.record_bounds)
            },
        )
    }
}

impl ProceduralCurveRow {
    /// The construction's identity.
    pub(crate) fn id_str(&self) -> &str {
        self.id.as_str()
    }

    /// The row a construction and its carrier state.
    pub(crate) fn new(curve: CurveId, procedural: &ProceduralCurve) -> Self {
        Self {
            id: procedural.id.clone(),
            curve,
            definition: procedural.definition.clone(),
            native_ref: procedural.native_ref.clone(),
        }
    }

    /// The carrier and the construction this row states.
    pub(crate) fn into_parts(self) -> (CurveId, ProceduralCurve) {
        (
            self.curve,
            ProceduralCurve {
                id: self.id,
                definition: self.definition,
                native_ref: self.native_ref,
            },
        )
    }
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_carrier_parameter_range,
    crate::topology::IncreasingParameterInterval,
    "parameter_range"
);
cadmpeg_core::named_optional_field!(deserialize_tertiary, bool, "tertiary");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
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

cadmpeg_core::named_optional_field!(deserialize_pcurve, PcurveGeometry, "pcurve");

cadmpeg_core::named_optional_field!(
    deserialize_parameter_range,
    DirectedParameterRange,
    "parameter_range"
);

cadmpeg_core::named_optional_field!(deserialize_record_bounds, RecordBounds, "record_bounds");

mod identity_rewrite;

mod serialization;

mod support_reversal;

cadmpeg_core::named_optional_field!(
    deserialize_procedural_surface_record_bounds,
    RecordBounds,
    "record_bounds"
);
cadmpeg_core::named_optional_field!(deserialize_self_intersect, bool, "self_intersect");
