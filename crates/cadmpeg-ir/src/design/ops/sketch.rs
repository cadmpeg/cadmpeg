// SPDX-License-Identifier: Apache-2.0
//! Sketches: sets of entities and the constraints between them, with a
//! placement.
//!
//! Planar sketches hold entities in sketch coordinates on a placed plane;
//! spatial sketches hold them in the coordinates of their definition. Both
//! share one entity and constraint vocabulary, parameterized by the
//! [`SketchSpace`]. Constraints are canonical over the solved geometry, and
//! dimension values are [`Dimension`]s. The regions of a sketch are
//! [`SketchRegion`](crate::design::selection::SketchRegion)s.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::num::NonZeroU32;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::shared::{RunSides, Spacing};
use crate::design::construction::{kinds, Construction, ScalarDimension};
use crate::design::dimension::Dimension;
use crate::design::evaluation::EvaluationStatus;
use crate::design::feature::LinkState;
use crate::design::ids::SpatialSketchConstraintId;
use crate::design::names::{InstanceIndex, NativeName};
use crate::design::options::{NativeOptions, ProjectionDirection};
use crate::design::selection::{OrderedSelection, PrincipalEntity, SetSelection};
use crate::features::geometry::FinitePoint3;
use crate::features::FeatureId;
use crate::geometry::pcurve::PcurveNurbs;
use crate::geometry::InlineCurve;
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength, PositiveReal};
use crate::sketches::{
    SketchConstraintId, SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId,
};
use crate::topology::IncreasingParameterInterval;
use crate::units::{FinitePoint2, UnitVector2, UnitVector3};

/// A planar sketch: entities in sketch coordinates on a placed plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Sketch {
    /// The sketch's id.
    pub id: SketchId,
    /// The feature whose definition holds the sketch.
    pub owner: FeatureId,
    /// The source's name for the sketch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The placement.
    pub plane: SketchPlane,
    /// The solve state and the status of the solved values.
    pub solve: SketchSolve,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
    /// The native record of the sketch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The placement of a planar sketch.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchPlane {
    /// The plane frame, as a construction. Its `resolved` is the solved
    /// frame, a cache.
    Placed(Box<Construction<kinds::Frame>>),
    /// A block definition: a sketch with no plane, whose entities block
    /// instances in other sketches place.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Block,
}

/// A spatial sketch: entities in the coordinates of its definition. It has
/// no placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SpatialSketch {
    /// The sketch's id.
    pub id: SpatialSketchId,
    /// The feature whose definition holds the sketch.
    pub owner: FeatureId,
    /// The source's name for the sketch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The solve state and the status of the solved values.
    pub solve: SketchSolve,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
    /// The native record of the sketch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// How far a sketch's constraints determine its geometry, and the status of
/// its solved values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchSolve {
    /// The source's record of how far the constraints determine the
    /// geometry. `None` means not stated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_state"
    )]
    pub state: Option<SolveState>,
    /// The status of the sketch's solved values: entity geometry and the
    /// resolved frame.
    pub status: EvaluationStatus,
}

/// The source's record of how far a sketch's constraints determine its
/// geometry.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SolveState {
    /// Fully constrained.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Full,
    /// Under-constrained.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Under,
    /// Not fully constrained: under-constrained or unsolvable, the source
    /// does not say which.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    NotFull,
    /// Over-constrained.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Over,
    /// The constraints contradict each other.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Inconsistent,
    /// A state whose meaning is not established.
    Native(NonBlankString),
}

/// The id, point, direction, and curve types of a sketch space. Each carries
/// the wire traits, so every record generic over a space derives them.
pub trait SketchSpace: PartialEq {
    /// The id of a sketch of the space.
    type Sketch: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
    /// The id of an entity of the space.
    type Entity: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
    /// The id of a constraint of the space.
    type Constraint: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
    /// A point of the space.
    type Point: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
    /// A direction of the space.
    type Direction: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
    /// A curve of the space.
    type Curve: Debug + Clone + PartialEq + Serialize + DeserializeOwned;
}

/// The space of a planar sketch: sketch coordinates on its plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Planar;

impl SketchSpace for Planar {
    type Sketch = SketchId;
    type Entity = SketchEntityId;
    type Constraint = SketchConstraintId;
    type Point = FinitePoint2;
    type Direction = UnitVector2;
    type Curve = PlanarCurve;
}

/// The space of a spatial sketch: the coordinates of its definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Spatial;

impl SketchSpace for Spatial {
    type Sketch = SpatialSketchId;
    type Entity = SpatialSketchEntityId;
    type Constraint = SpatialSketchConstraintId;
    type Point = FinitePoint3;
    type Direction = UnitVector3;
    type Curve = SpatialCurve;
}

/// An entity of a sketch: geometry with its defining form, its roles, and
/// its flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "S: JsonSchema, S::Sketch: JsonSchema, S::Entity: JsonSchema, S::Curve: JsonSchema, S::Direction: JsonSchema"
    )
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchEntity<S: SketchSpace> {
    /// The entity's id.
    pub id: S::Entity,
    /// The sketch that holds the entity.
    pub sketch: S::Sketch,
    /// The entity's curve, free or with a defining form.
    pub definition: EntityDefinition<S>,
    /// What the entity bounds or helps define: `{ of: L, role: Start }` makes
    /// a point the start of line L.
    pub roles: Vec<EntityRole<S>>,
    /// Marks geometry that no profile uses. `None` where the source flag is
    /// unread.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_construction"
    )]
    pub construction: Option<bool>,
    /// Marks the line the source uses as the axis of revolution and of
    /// doubled (diameter) dimensions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_centerline"
    )]
    pub centerline: Option<bool>,
    /// The source's identity for the entity where it is no record id, for a
    /// writer to the same codec and for name derivation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native"
    )]
    pub native: Option<NativeName>,
    /// Source flags whose meaning is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source settings that only a writer of the codec writes.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
    /// The native record of the entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// An entity's curve: free geometry that the constraints position, or a
/// defining form with its solved curve.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "S: JsonSchema, S::Entity: JsonSchema, S::Curve: JsonSchema, S::Direction: JsonSchema"
    )
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum EntityDefinition<S: SketchSpace> {
    /// Geometry the constraints position: the curve is the solved value and
    /// the cache of the constraints; the entity's kind is the curve's kind.
    Free(S::Curve),
    /// A defining form beyond the curve.
    Defined {
        /// The defining form.
        form: Box<EntityForm<S>>,
        /// The form's cache. `None` only where the source stores no solved
        /// value.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solved"
        )]
        solved: Option<S::Curve>,
    },
}

/// The defining form of a `Defined` entity.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema, S::Direction: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum EntityForm<S: SketchSpace> {
    /// A spline through its fit points.
    Interpolated(InterpolatedSpline<S>),
    /// A conic arc from its `Start` to its `End` point, tangent there to the
    /// lines toward its `Apex` point.
    Conic {
        /// The shape ratio.
        rho: Dimension<FiniteReal>,
    },
    /// An entity computed from model geometry.
    Derived(Box<Derivation>),
    /// Text.
    Text(Box<SketchText<S>>),
    /// A placed block definition.
    BlockInstance(BlockInstance),
    /// An entity of a block definition that the host sketch shows, with its
    /// placed curve in `solved`.
    Instanced {
        /// The block instance.
        instance: S::Entity,
        /// The entity of the block definition.
        of: SketchEntityId,
    },
    /// A sketch-local coordinate frame: a point entity, its `solved` the
    /// frame's origin.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Frame,
    /// A form whose meaning is not established.
    Native(NonBlankString),
}

/// What an entity bounds or helps define.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct EntityRole<S: SketchSpace> {
    /// The entity the role is of.
    pub of: S::Entity,
    /// The role.
    pub role: DefiningRole,
}

/// The role of a point or a helper in the entity it bounds or helps define.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DefiningRole {
    /// The start point.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Start,
    /// The end point.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    End,
    /// The center.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Center,
    /// A conic's apex.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Apex,
    /// A focus, by index.
    Focus(u8),
    /// An ellipse's major axis line.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    MajorAxis,
    /// An ellipse's minor axis line.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    MinorAxis,
    /// The x axis line of a `Frame` entity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    XAxis,
    /// The y axis line of a `Frame` entity.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    YAxis,
    /// The fit point of an interpolated spline, by index.
    Fit(u32),
    /// A spline pole, by index.
    Pole(u32),
    /// A spline knot, by index.
    Knot(u32),
    /// A spline handle line at a fit point, by index.
    Handle(u32),
    /// A text anchor.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Anchor,
    /// A block insertion point.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Insertion,
    /// A role whose meaning is not established.
    Native(NonBlankString),
}

/// A 2D carrier in sketch coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PlanarCurve {
    /// A point.
    Point(FinitePoint2),
    /// A line segment.
    Line {
        /// The start.
        start: FinitePoint2,
        /// The end.
        end: FinitePoint2,
    },
    /// An unbounded line.
    InfiniteLine {
        /// A point on the line.
        through: FinitePoint2,
        /// The line's direction.
        direction: UnitVector2,
    },
    /// A full circle.
    Circle {
        /// The center.
        center: FinitePoint2,
        /// The radius.
        radius: PositiveLength,
    },
    /// A circular arc, counterclockwise from `start` to `end`.
    Arc {
        /// The center.
        center: FinitePoint2,
        /// The radius.
        radius: PositiveLength,
        /// The start angle.
        start: Angle,
        /// The end angle.
        end: Angle,
    },
    /// An ellipse.
    Ellipse {
        /// The center.
        center: FinitePoint2,
        /// The major axis direction.
        major: UnitVector2,
        /// The major and minor radii.
        radii: [PositiveLength; 2],
        /// The parameter range. `None` is the full curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_range"
        )]
        range: Option<[Angle; 2]>,
    },
    /// A parabola.
    Parabola {
        /// The vertex.
        vertex: FinitePoint2,
        /// The axis direction.
        axis: UnitVector2,
        /// The focal distance.
        focal: PositiveLength,
        /// The parameter range. `None` is the full curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_range"
        )]
        range: Option<[FiniteReal; 2]>,
    },
    /// A hyperbola.
    Hyperbola {
        /// The center.
        center: FinitePoint2,
        /// The major axis direction.
        major: UnitVector2,
        /// The major and minor radii.
        radii: [PositiveLength; 2],
        /// The parameter range. `None` is the full curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_range"
        )]
        range: Option<[FiniteReal; 2]>,
    },
    /// A NURBS curve.
    Nurbs(PcurveNurbs),
    /// A non-planar result inside a planar sketch; only a derived entity has
    /// one.
    Spatial(SpatialCurve),
}

/// A carrier in the coordinates of a spatial sketch's definition.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpatialCurve {
    /// A point.
    Point(FinitePoint3),
    /// A curve carrier with the interval of its parameter that the piece
    /// uses.
    Bounded(BoundedCurve),
}

/// A curve carrier and the interval of its own parameter that a piece uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BoundedCurve {
    /// The carrier.
    pub carrier: InlineCurve,
    /// The interval of the carrier's parameter. `None` is the carrier's whole
    /// domain; a piece the source bounds inside that domain, an arc
    /// included, states it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_range"
    )]
    pub range: Option<IncreasingParameterInterval>,
}

/// A spline through its fit points, the point entities with `Fit(0)` to
/// `Fit(n−1)` roles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Direction: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct InterpolatedSpline<S: SketchSpace> {
    /// The degree.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_degree"
    )]
    pub degree: Option<NonZeroU32>,
    /// Whether the spline is closed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_closed"
    )]
    pub closed: Option<bool>,
    /// The tangent and curvature conditions at fit point `i`, by `i`.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeSet<(u32, SplineHandle<S>)>")
    )]
    pub handles: BTreeMap<u32, SplineHandle<S>>,
}

/// The tangent and curvature conditions at one fit point of a spline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Direction: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SplineHandle<S: SketchSpace> {
    /// The tangent direction. `None` where a `Handle(i)` helper and its
    /// relations state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<S::Direction>,
    /// The tangent weighting, which a dimension can drive.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_magnitude"
    )]
    pub magnitude: Option<Dimension<FiniteReal>>,
    /// The curvature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curvature"
    )]
    pub curvature: Option<FiniteReal>,
}

/// An entity computed from model geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Derivation {
    /// How the entity is computed from its source.
    pub mode: DerivationMode,
    /// The model entities.
    pub source: OrderedSelection,
    /// The entity's number among the entities that one source produces, in
    /// the order the source stores their records. `None` where the source
    /// stores one model entity per output.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_piece"
    )]
    pub piece: Option<u32>,
    /// The source's record of associativity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_link"
    )]
    pub link: Option<LinkState>,
}

/// How a derived entity is computed from its source.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DerivationMode {
    /// Projects the source onto the sketch plane.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Project,
    /// Copies the source curve unprojected.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Include,
    /// Intersects the source with the sketch plane.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Intersect,
    /// Takes the source's outline seen along the normal.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Silhouette,
    /// Projects and offsets the source along the projected curve's left
    /// normal.
    Offset {
        /// The signed offset distance.
        distance: Dimension<Length>,
    },
    /// Projects the source onto faces along a direction, so the result lies
    /// on those faces.
    OntoFaces {
        /// The faces.
        faces: SetSelection,
        /// The projection direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<Box<ProjectionDirection>>,
    },
    /// A derivation whose meaning is not established.
    Native(NonBlankString),
}

/// Text in a sketch, with its layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchText<S: SketchSpace> {
    /// The content.
    pub text: TextContent,
    /// A frame of entities or a curve the text is fitted to. `None` places
    /// the text at its `Anchor` point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_layout"
    )]
    pub layout: Option<TextLayout<S>>,
}

/// The content of a text, with its font and alignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextContent {
    /// The text.
    pub content: NonBlankString,
    /// The font.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_font"
    )]
    pub font: Option<NonBlankString>,
    /// The cap height, the distance from the baseline to the top of a flat
    /// capital letter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height"
    )]
    pub height: Option<Dimension<Length>>,
    /// The scale of the nominal glyph width.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width_factor"
    )]
    pub width_factor: Option<Dimension<FiniteReal>>,
    /// The rotation from the sketch x axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
    /// The horizontal alignment against the box `horizontal_box` names.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_horizontal"
    )]
    pub horizontal: Option<TextAlign>,
    /// The vertical alignment against the box `vertical_box` names.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical"
    )]
    pub vertical: Option<TextAlign>,
    /// The box that horizontal alignment measures.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_horizontal_box"
    )]
    pub horizontal_box: Option<TextHorizontalBox>,
    /// The box that vertical alignment measures.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vertical_box"
    )]
    pub vertical_box: Option<TextVerticalBox>,
    /// A distance added between consecutive characters.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tracking"
    )]
    pub tracking: Option<Dimension<Length>>,
    /// The shear of the glyphs, x' = x + y tan(`oblique`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_oblique"
    )]
    pub oblique: Option<Dimension<Angle>>,
    /// `Some(false)` makes the glyphs wires, not faces.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_filled"
    )]
    pub filled: Option<Dimension<bool>>,
}

/// The alignment of a text against a box.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TextAlign {
    /// The box's start; vertically against `CapHeight`, the baseline.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Start,
    /// The box's center.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Center,
    /// The box's end.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    End,
    /// The baseline. It does not occur with `CapHeight`.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Baseline,
    /// An alignment whose meaning is not established.
    Native(NonBlankString),
}

/// The box that a text's horizontal alignment measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextHorizontalBox {
    /// The ink's extent.
    Ink,
    /// The pen's extent from its origin, leading space and side bearing
    /// included.
    Pen,
}

/// The box that a text's vertical alignment measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextVerticalBox {
    /// The ink's extent.
    Ink,
    /// The band from the baseline to the cap height.
    CapHeight,
}

/// A frame or a curve that a text is fitted to.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum TextLayout<S: SketchSpace> {
    /// Fitted to a frame of entities.
    Frame(Vec<S::Entity>),
    /// Laid along a curve.
    Path {
        /// The curve.
        curve: S::Entity,
        /// Whether the text runs against the curve's direction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reversed"
        )]
        reversed: Option<bool>,
    },
}

/// A block definition placed at its `Insertion` point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlockInstance {
    /// The block definition.
    pub block: SketchId,
    /// The rotation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
    /// The scale.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale"
    )]
    pub scale: Option<Dimension<FiniteReal>>,
}

/// A constraint of a sketch: a relation or a dimension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "S: JsonSchema, S::Sketch: JsonSchema, S::Entity: JsonSchema, S::Constraint: JsonSchema"
    )
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchConstraint<S: SketchSpace> {
    /// The constraint's id.
    pub id: S::Constraint,
    /// The sketch that holds the constraint.
    pub sketch: S::Sketch,
    /// The relation.
    pub relation: SketchRelation<S>,
    /// A relation's own name, and the name of a literal dimension that the
    /// source names and no parameter holds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The source's activity flag, which a formula can drive.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_active"
    )]
    pub active: Option<Dimension<bool>>,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source settings that only a writer of the codec writes.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
    /// The native record of the constraint.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// An operand of a sketch relation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum Operand<S: SketchSpace> {
    /// A sketch entity; a point is a point entity.
    Entity(S::Entity),
    /// One of the sketch's own axes or its origin. It does not occur in a
    /// spatial sketch.
    Frame(PrincipalEntity),
    /// A selection of model geometry.
    Model(Box<OrderedSelection>),
}

/// A geometric relation or a dimension between sketch operands.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum SketchRelation<S: SketchSpace> {
    /// Two points made equal, or a point put on a curve.
    Coincident {
        /// The first operand.
        a: Operand<S>,
        /// The second operand.
        b: Operand<S>,
    },
    /// Circles, arcs, or ellipses with one center.
    Concentric {
        /// The first operand.
        a: Operand<S>,
        /// The second operand.
        b: Operand<S>,
    },
    /// Two lines on one line.
    Collinear {
        /// The first operand.
        a: Operand<S>,
        /// The second operand.
        b: Operand<S>,
    },
    /// Two parallel lines.
    Parallel {
        /// The first operand.
        a: Operand<S>,
        /// The second operand.
        b: Operand<S>,
    },
    /// Two perpendicular lines.
    Perpendicular {
        /// The first operand.
        a: Operand<S>,
        /// The second operand.
        b: Operand<S>,
    },
    /// Two curves tangent.
    Tangent {
        /// The first curve.
        a: Operand<S>,
        /// The second curve.
        b: Operand<S>,
        /// Where the curves touch. `None` means anywhere.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_contact"
        )]
        contact: Option<Contact<S>>,
    },
    /// Two curves curvature-continuous (G2).
    Curvature {
        /// The first curve.
        a: Operand<S>,
        /// The second curve.
        b: Operand<S>,
        /// Where the curves touch. `None` means anywhere.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_contact"
        )]
        contact: Option<Contact<S>>,
    },
    /// One horizontal line, or two points at one height.
    Horizontal {
        /// The line or the first point.
        a: Operand<S>,
        /// The second point.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_b"
        )]
        b: Option<Operand<S>>,
    },
    /// One vertical line, or two points one above the other.
    Vertical {
        /// The line or the first point.
        a: Operand<S>,
        /// The second point.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_b"
        )]
        b: Option<Operand<S>>,
    },
    /// A point at the middle of a bounded curve.
    Midpoint {
        /// The point.
        point: S::Entity,
        /// The curve.
        of: Operand<S>,
    },
    /// Two operands mirrored about a line or a point.
    Symmetric {
        /// The first operand.
        a: Operand<S>,
        /// The second operand.
        b: Operand<S>,
        /// The mirror line or point.
        about: Operand<S>,
    },
    /// Equal length for lines and equal radius for circular entities, over
    /// two or more members.
    Equal {
        /// The members.
        members: Vec<Operand<S>>,
    },
    /// The distance between the first pair equals the distance between the
    /// second.
    EqualDistance {
        /// The first pair.
        first: [Operand<S>; 2],
        /// The second pair.
        second: [Operand<S>; 2],
    },
    /// An operand locked in place.
    Fixed {
        /// The operand.
        of: Operand<S>,
    },
    /// A distance from `a` to `b`.
    Distance {
        /// The operand measured from.
        a: Operand<S>,
        /// The operand measured to.
        b: Operand<S>,
        /// The axis the distance is measured along.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_along"
        )]
        along: Option<DistanceAxis<S>>,
        /// The value is twice the distance, measured across a centerline.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_doubled"
        )]
        doubled: Option<bool>,
        /// The value.
        value: Dimension<Length>,
    },
    /// An angle.
    Angle {
        /// What the angle is measured between.
        between: AngleOperands<S>,
        /// Which angle the value is, where the source records one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sector"
        )]
        sector: Option<AngleSector>,
        /// The value.
        value: Dimension<Angle>,
    },
    /// A radial dimension of a circle, arc, or ellipse.
    Radial {
        /// The entity.
        of: S::Entity,
        /// What the value measures.
        measure: RadialMeasure,
        /// The value.
        value: Dimension<Length>,
    },
    /// The total arc length of its curves.
    Length {
        /// The curves.
        of: Vec<S::Entity>,
        /// The value.
        value: Dimension<Length>,
    },
    /// Results at a signed distance from their sources.
    Offset(SketchOffset<S>),
    /// Copies of seed entities.
    Pattern(Box<SketchPattern<S>>),
    /// A polygon.
    Polygon(SketchPolygon<S>),
    /// A slot.
    Slot(SketchSlot<S>),
    /// A point where a curve that does not lie in the sketch plane crosses
    /// that plane.
    Pierce {
        /// The point.
        point: S::Entity,
        /// The curve.
        curve: Operand<S>,
    },
    /// The rational weight of the spline pole whose `Pole(i)` helper is
    /// `pole`.
    Weight {
        /// The pole helper.
        pole: S::Entity,
        /// The weight.
        value: Dimension<PositiveReal>,
    },
    /// Each line held at its solved length.
    FixedLength {
        /// The lines.
        of: Vec<S::Entity>,
    },
    /// Each line held at its solved angle to the sketch's x axis.
    FixedAngle {
        /// The lines.
        of: Vec<S::Entity>,
    },
    /// A relation whose meaning is not established, with its operands and
    /// values.
    Native {
        /// The source's kind.
        kind: NonBlankString,
        /// The operands.
        operands: Vec<Operand<S>>,
        /// The values.
        values: Vec<ScalarDimension>,
    },
}

/// Where two tangent curves touch.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum Contact<S: SketchSpace> {
    /// At a point lying on both.
    Via(S::Entity),
    /// At an end of each, which also makes the two ends coincide.
    Ends([S::Entity; 2]),
}

/// The axis a sketch distance is measured along.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum DistanceAxis<S: SketchSpace> {
    /// The Euclidean distance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Aligned,
    /// The signed sketch-coordinate difference `b − a` in x.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Horizontal,
    /// The signed sketch-coordinate difference `b − a` in y.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Vertical,
    /// Parallel to a line or axis.
    Along(Operand<S>),
}

/// What a sketch angle is measured between.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum AngleOperands<S: SketchSpace> {
    /// From the first line's direction to the second's.
    Between([Operand<S>; 2]),
    /// The angle an arc or elliptic arc subtends.
    Sweep(S::Entity),
    /// The angle at the second point from the direction toward the first
    /// point to the direction toward the third.
    Points([S::Entity; 3]),
}

/// Which angle a sketch angle's value is.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AngleSector {
    /// The non-reflex angle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Minor,
    /// The reflex angle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Major,
    /// The angle swept from the first operand's direction to the second's,
    /// in [0, 2π).
    Directed {
        /// The second operand's direction is reversed.
        second_reversed: bool,
        /// The angle is swept clockwise about the sketch normal.
        clockwise: bool,
    },
    /// A sector whose meaning is not established.
    Native(NonBlankString),
}

/// What a radial dimension measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RadialMeasure {
    /// The radius.
    Radius,
    /// The diameter.
    Diameter,
    /// An ellipse's major radius.
    MajorRadius,
    /// An ellipse's minor radius.
    MinorRadius,
}

/// Sketch entities offset from other sketch entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchOffset<S: SketchSpace> {
    /// (source, result) pairs: the result lies at the signed `distance`
    /// along the source's left normal.
    pub pairs: Vec<[S::Entity; 2]>,
    /// The signed distance.
    pub distance: Dimension<Length>,
}

/// A polygon: lines held by one relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchPolygon<S: SketchSpace> {
    /// The center point.
    pub center: S::Entity,
    /// The edges.
    pub edges: Vec<S::Entity>,
    /// The construction circle the relation holds the polygon to, where the
    /// source stores one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_circle"
    )]
    pub circle: Option<S::Entity>,
    /// `Some(true)` holds the circle tangent to every edge, `Some(false)`
    /// puts every vertex on it, and `None` states that the source does not
    /// say which.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inscribed"
    )]
    pub inscribed: Option<bool>,
}

/// A slot: lines and arcs held by one relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchSlot<S: SketchSpace> {
    /// The kind of slot.
    pub kind: SlotKind,
    /// The two center points.
    pub centers: [S::Entity; 2],
    /// The edges.
    pub edges: Vec<S::Entity>,
}

/// The kind of a sketch slot.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SlotKind {
    /// A straight slot by its ends.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Straight,
    /// A straight slot by its center.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    CenterStraight,
    /// An arc slot by its ends.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Arc,
    /// An arc slot by its center.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    CenterArc,
    /// A slot whose meaning is not established.
    Native(NonBlankString),
}

/// A sketch pattern: constrained copies of seed entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchPattern<S: SketchSpace> {
    /// The seed entities.
    pub seeds: Vec<S::Entity>,
    /// The layout.
    pub layout: SketchPatternLayout<S>,
    /// The instances, each with its entities in seed order at its index.
    pub instances: Vec<SketchPatternInstance<S>>,
}

/// The layout of a sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum SketchPatternLayout<S: SketchSpace> {
    /// One or two directions.
    Linear {
        /// The directions.
        directions: Vec<SketchPatternAxis<S>>,
    },
    /// A turn about a center.
    Circular {
        /// The center.
        center: Operand<S>,
        /// The angular spacing.
        spacing: Spacing<Angle>,
        /// Where the run places its instances.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sides"
        )]
        sides: Option<RunSides<Angle>>,
    },
    /// A layout whose meaning is not established.
    Native(NonBlankString),
}

/// One direction of a linear sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchPatternAxis<S: SketchSpace> {
    /// The direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<SketchPatternDirection<S>>,
    /// The spacing.
    pub spacing: Spacing<Length>,
    /// Where the run places its instances.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sides"
    )]
    pub sides: Option<RunSides<Length>>,
}

/// The direction of a linear sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum SketchPatternDirection<S: SketchSpace> {
    /// Along a line or axis.
    Along {
        /// The line or axis.
        line: Operand<S>,
        /// Whether the direction runs against the line's.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_reversed"
        )]
        reversed: Option<bool>,
    },
    /// At an angle from the sketch x axis.
    Angle(Dimension<Angle>),
}

/// One instance of a sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "S: JsonSchema, S::Entity: JsonSchema")
)]
#[serde(deny_unknown_fields, bound = "")]
pub struct SketchPatternInstance<S: SketchSpace> {
    /// The instance's index.
    pub index: InstanceIndex,
    /// The instance's entities, in seed order. A suppressed instance has
    /// none.
    pub entities: Vec<S::Entity>,
    /// `Some(true)` for a suppressed instance.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_suppressed"
    )]
    pub suppressed: Option<bool>,
}

cadmpeg_core::named_optional_field!(deserialize_active<T>, T, "active");
cadmpeg_core::named_optional_field!(deserialize_along<T>, T, "along");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_b<T>, T, "b");
cadmpeg_core::named_optional_field!(deserialize_centerline<T>, T, "centerline");
cadmpeg_core::named_optional_field!(deserialize_circle<T>, T, "circle");
cadmpeg_core::named_optional_field!(deserialize_closed<T>, T, "closed");
cadmpeg_core::named_optional_field!(deserialize_construction<T>, T, "construction");
cadmpeg_core::named_optional_field!(deserialize_contact<T>, T, "contact");
cadmpeg_core::named_optional_field!(deserialize_curvature<T>, T, "curvature");
cadmpeg_core::named_optional_field!(deserialize_degree<T>, T, "degree");
cadmpeg_core::named_optional_field!(deserialize_direction<T>, T, "direction");
cadmpeg_core::named_optional_field!(deserialize_doubled<T>, T, "doubled");
cadmpeg_core::named_optional_field!(deserialize_filled<T>, T, "filled");
cadmpeg_core::named_optional_field!(deserialize_font<T>, T, "font");
cadmpeg_core::named_optional_field!(deserialize_height<T>, T, "height");
cadmpeg_core::named_optional_field!(deserialize_horizontal<T>, T, "horizontal");
cadmpeg_core::named_optional_field!(deserialize_horizontal_box<T>, T, "horizontal_box");
cadmpeg_core::named_optional_field!(deserialize_inscribed<T>, T, "inscribed");
cadmpeg_core::named_optional_field!(deserialize_layout<T>, T, "layout");
cadmpeg_core::named_optional_field!(deserialize_link<T>, T, "link");
cadmpeg_core::named_optional_field!(deserialize_magnitude<T>, T, "magnitude");
cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_native<T>, T, "native");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_oblique<T>, T, "oblique");
cadmpeg_core::named_optional_field!(deserialize_piece<T>, T, "piece");
cadmpeg_core::named_optional_field!(deserialize_range<T>, T, "range");
cadmpeg_core::named_optional_field!(deserialize_reversed<T>, T, "reversed");
cadmpeg_core::named_optional_field!(deserialize_scale<T>, T, "scale");
cadmpeg_core::named_optional_field!(deserialize_sector<T>, T, "sector");
cadmpeg_core::named_optional_field!(deserialize_sides<T>, T, "sides");
cadmpeg_core::named_optional_field!(deserialize_solved<T>, T, "solved");
cadmpeg_core::named_optional_field!(deserialize_state<T>, T, "state");
cadmpeg_core::named_optional_field!(deserialize_suppressed<T>, T, "suppressed");
cadmpeg_core::named_optional_field!(deserialize_tracking<T>, T, "tracking");
cadmpeg_core::named_optional_field!(deserialize_vertical<T>, T, "vertical");
cadmpeg_core::named_optional_field!(deserialize_vertical_box<T>, T, "vertical_box");
cadmpeg_core::named_optional_field!(deserialize_width_factor<T>, T, "width_factor");
