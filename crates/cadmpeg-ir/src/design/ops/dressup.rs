// SPDX-License-Identifier: Apache-2.0
//! Dress-up and holes (§6.4).
//!
//! A dress-up operation changes bodies that earlier features made: it rounds
//! or bevels edges, hollows, tapers, offsets, or thickens faces, drills holes,
//! adds threads, or embosses a profile. The family is [`Fillet`], [`Chamfer`],
//! [`ShellFeature`], [`Draft`], [`Thicken`], [`Offset`], [`Hole`], [`Thread`],
//! [`Emboss`], [`Dome`], and [`Decal`], a decal binding an image to faces with
//! no change of geometry.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::assets::AssetId;
use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::ops::shared::{Interpolation, Limit, OffsetJoin};
use crate::design::options::{
    Boolean, Hand, NativeOptions, PathRef, ProfileRef, SheetOutput, Targets, Thickness,
};
use crate::design::selection::{OrderedSelection, SetSelection};
use crate::features::FeatureId;
use crate::scalar::{
    Fraction, InteriorAngle, Length, NonNegativeLength, PositiveLength, PositiveReal, SlopeAngle,
};

#[cfg(test)]
mod tests;

/// A scalar that varies along the edge chain of one set: a variable fillet
/// radius or a variable draft angle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    deny_unknown_fields,
    bound(
        serialize = "Q: Serialize",
        deserialize = "Q: ::serde::de::DeserializeOwned"
    )
)]
pub struct Stations<Q> {
    /// The stations, each a position on the chain and the value there.
    pub points: Vec<ValueStation<Q>>,
    /// How the value runs between stations.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_law"
    )]
    pub law: Option<Interpolation>,
    /// The vertex at fraction 0 of the chain, where the source stores one.
    /// `None` states that the source stores no such vertex: a fillet's chain
    /// then starts at the start vertex of its set's first edge, in that
    /// edge's own direction, and a draft's fixed edges, which have no order,
    /// state no fraction 0.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chain_start"
    )]
    pub chain_start: Option<SetSelection>,
}

/// One station of a varying value: where it lies on the chain and the value
/// there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    deny_unknown_fields,
    bound(
        serialize = "Q: Serialize",
        deserialize = "Q: ::serde::de::DeserializeOwned"
    )
)]
pub struct ValueStation<Q> {
    /// Where the station lies.
    pub at: StationAt,
    /// The value at the station.
    pub value: Dimension<Q>,
}

/// Where a station lies on the chain.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StationAt {
    /// At a fraction of the whole chain, or of one edge in the edge's own
    /// direction.
    Parameter {
        /// What the fraction runs along.
        along: Along,
        /// The fraction, 0 at the start and 1 at the end.
        fraction: Dimension<Fraction>,
    },
    /// At a constructed point on the chain: a vertex, a datum point, or a
    /// point at a distance along an edge.
    Point(Construction<kinds::Point>),
    /// A station position whose meaning is not established. The station's
    /// value stays typed.
    Native(NonBlankString),
}

/// What a station's fraction runs along.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Along {
    /// The whole chain of the set.
    Chain {},
    /// One edge, in the edge's own direction.
    Edge(OrderedSelection),
}

/// Rounds edges or blends between face sets. A fillet holds one or more
/// sets, each with its own size and profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Fillet {
    /// The sets, each with its own size and profile.
    pub sets: Vec<FilletSet>,
    /// The controls the sets share.
    pub controls: BlendControls,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// One set of a fillet.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilletSet {
    /// Rounds the edges the selection reaches, in source order.
    Edges {
        /// The edges, in source order.
        edges: OrderedSelection,
        /// The cross-section.
        section: FilletSection,
        /// The spine that orients the cross-sections.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_spine"
        )]
        spine: Option<PathRef>,
        /// The hold lines that bound the blend on each side, where the source
        /// states them: the first on the side whose face loop traverses the
        /// edge in the edge's own direction, the second on the other.
        hold: [Option<SetSelection>; 2],
    },
    /// Rounds between two face sets.
    Faces {
        /// The first face set.
        first: FaceSide,
        /// The second face set.
        second: Box<FaceSide>,
        /// The cross-section.
        section: FilletSection,
        /// The spine that orients the cross-sections.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_spine"
        )]
        spine: Option<PathRef>,
        /// The help point that chooses among solutions.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_near"
        )]
        near: Option<Construction<kinds::Point>>,
        /// Whether the support faces are trimmed to the blend.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_trim"
        )]
        trim: Option<Dimension<bool>>,
    },
    /// Replaces a center face set by a blend tangent to the two side sets.
    FullRound {
        /// The first side.
        first: RoundSide,
        /// The center faces the blend replaces.
        center: SetSelection,
        /// The second side.
        second: RoundSide,
    },
}

/// One face set of a face fillet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FaceSide {
    /// The faces.
    pub faces: SetSelection,
    /// A hold line or curve that bounds the blend on this side.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hold"
    )]
    pub hold: Option<SetSelection>,
}

/// One side of a full round.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoundSide {
    /// The source computes the side from the center faces.
    Automatic {},
    /// The selected faces.
    Faces(SetSelection),
}

/// The cross-section of a fillet set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FilletSection {
    /// The size.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fillet_size"
    )]
    pub size: Option<FilletSize>,
    /// The cross-section shape.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_profile"
    )]
    pub profile: Option<FilletProfile>,
    /// The source's tangency weight of the section, whatever its profile. A
    /// weight other than 1 makes the section non-circular.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_weight"
    )]
    pub weight: Option<Dimension<PositiveReal>>,
}

/// The size of a fillet set.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FilletSize {
    /// A constant rolling-ball radius.
    Radius(Dimension<PositiveLength>),
    /// A constant chord width between the support boundaries.
    Chord(Dimension<PositiveLength>),
    /// Distinct setbacks on the two supports. `first` lies on the face whose
    /// loop traverses the edge in the edge's own direction for an edge set,
    /// and on the first face set for a face set.
    Offsets {
        /// The setback on the first support.
        first: Dimension<PositiveLength>,
        /// The setback on the second support.
        second: Dimension<PositiveLength>,
    },
    /// A radius by stations along the set's chain.
    Variable(Stations<NonNegativeLength>),
    /// The hold lines alone fix the width.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Held,
    /// The conic profile alone fixes the width.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ByProfile,
    /// A size form whose meaning is not established. The rest of the round
    /// stays typed.
    Native(NonBlankString),
}

/// The cross-section shape of a fillet set.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FilletProfile {
    /// A circular section.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Circular,
    /// A conic section sized by its shape.
    Conic(ConicShape),
    /// A curvature-continuous (G2) section.
    CurvatureContinuous {
        /// The source's shape factor of the section, from 0 to 0.95.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_shape_factor"
        )]
        shape: Option<Dimension<Fraction>>,
    },
    /// A source section whose meaning is not established.
    Native(NonBlankString),
}

/// How a conic fillet section is sized.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConicShape {
    /// The conic ratio, with the width from the set's size.
    Rho {
        /// The conic ratio. `None` where the source does not state it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rho"
        )]
        rho: Option<Dimension<Fraction>>,
    },
    /// The boundary from the set's size, and the center distance.
    BoundaryAndCenter {
        /// The center distance.
        center: Dimension<PositiveLength>,
    },
    /// The center distance and the conic ratio, which fix the width.
    CenterAndRho {
        /// The center distance.
        center: Dimension<PositiveLength>,
        /// The conic ratio.
        rho: Dimension<Fraction>,
    },
    /// The radius at the shoulder point of a symmetric fillet.
    ShoulderRadius(Dimension<PositiveLength>),
    /// A source conic sizing whose meaning is not established.
    Native(NonBlankString),
}

/// Bevels edges. A chamfer holds one or more sets, each with its own size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Chamfer {
    /// The sets.
    pub sets: Vec<ChamferSet>,
    /// The controls the sets share.
    pub controls: BlendControls,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// One set of a chamfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ChamferSet {
    /// The edges.
    pub edges: SetSelection,
    /// The size.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chamfer_size"
    )]
    pub size: Option<ChamferSize>,
    /// The reference face. Where it is `None`, the reference side is the face
    /// whose loop traverses the edge in the edge's own direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reference"
    )]
    pub reference: Option<SetSelection>,
    /// Whether the reference side is swapped.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flip"
    )]
    pub flip: Option<Dimension<bool>>,
    /// Whether the distances run along the faces or offset them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chamfer_measure"
    )]
    pub measure: Option<ChamferMeasure>,
}

/// The size of a chamfer set. A first distance lies on the reference side.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ChamferSize {
    /// An equal setback on both faces.
    Distance(Dimension<PositiveLength>),
    /// A setback on each face.
    Distances {
        /// The setback on the reference side.
        first: Dimension<PositiveLength>,
        /// The setback on the other side.
        second: Dimension<PositiveLength>,
    },
    /// A setback on the reference side and the angle from the reference
    /// face.
    DistanceAngle {
        /// The setback on the reference side.
        distance: Dimension<PositiveLength>,
        /// The angle from the reference face.
        angle: Dimension<InteriorAngle>,
    },
    /// The chamfer face's width across the edge and its angle from the
    /// reference face.
    WidthAngle {
        /// The width of the chamfer face across the edge.
        width: Dimension<PositiveLength>,
        /// The angle from the reference face.
        angle: Dimension<InteriorAngle>,
    },
    /// The distance from the edge where the two faces meet to the chamfer
    /// face, and the angle from the reference face.
    HeightAngle {
        /// The distance from the edge to the chamfer face.
        height: Dimension<PositiveLength>,
        /// The angle from the reference face.
        angle: Dimension<InteriorAngle>,
    },
    /// Cuts one vertex where three edges of one convexity meet by the plane
    /// through the three points at its distances from the vertex along those
    /// edges, each an arc length on its edge, one per edge.
    Vertex(Vec<EdgeSetback>),
    /// A source form whose meaning is not established. Its values are native
    /// options under the source's own parameter names.
    Native(NonBlankString),
}

/// What a chamfer's distances measure.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ChamferMeasure {
    /// The distances run along the faces.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    AlongFaces,
    /// The distances offset the faces.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    OffsetFaces,
    /// A source measure whose meaning is not established.
    Native(NonBlankString),
}

/// The controls that a fillet's or a chamfer's sets share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlendControls {
    /// The feature's choice of how blends meet at a vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_corner_style"
    )]
    pub corner_style: Option<CornerStyle>,
    /// The vertices where blended edges meet, each with its treatment.
    pub corners: Vec<Corner>,
    /// The elements or points at which the blend stops.
    pub limits: Vec<BlendLimit>,
    /// Edges the blend leaves intact by rolling along them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_keep"
    )]
    pub keep: Option<SetSelection>,
    /// How the sets meet.
    pub transitions: Vec<Transition>,
    /// What a blend does where it runs past a neighbouring edge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_overflow"
    )]
    pub overflow: Option<Overflow>,
    /// `Some(true)` keeps in the result a cut or boss feature whose faces
    /// the blend would cover; `Some(false)` lets the blend cover it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_keep_features"
    )]
    pub keep_features: Option<Dimension<bool>>,
    /// `Some(true)` blends the corner where two filleted edges meet
    /// smoothly, in place of a sharp juncture.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_round_corners"
    )]
    pub round_corners: Option<Dimension<bool>>,
}

/// How blends meet at a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CornerStyle {
    /// A rolling-ball corner.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    RollingBall,
    /// A setback corner.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Setback,
    /// A source corner style whose meaning is not established.
    Native(NonBlankString),
}

/// The treatment of a vertex where blended edges meet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Corner {
    /// The vertex.
    pub vertex: SetSelection,
    /// The distances from the vertex at which each blend starts its corner
    /// patch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_setbacks"
    )]
    pub setbacks: Option<Setbacks>,
}

/// The distances from a vertex at which the blends start their corner
/// patch.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Setbacks {
    /// One distance for every edge at the vertex.
    Uniform(Dimension<NonNegativeLength>),
    /// A distance for each edge.
    PerEdge(Vec<EdgeSetback>),
}

/// A distance along one edge from a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EdgeSetback {
    /// The edge.
    pub edge: SetSelection,
    /// The distance from the vertex along the edge.
    pub distance: Dimension<NonNegativeLength>,
}

/// Stops a blend at an element or at a point on the chain. The blend keeps
/// the side the element's normal points to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlendLimit {
    /// Where the blend stops.
    pub at: LimitAt,
    /// Whether the blend keeps the other side.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reversed"
    )]
    pub reversed: Option<Dimension<bool>>,
}

/// Where a blend stops.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LimitAt {
    /// At an element: a plane, face, surface, or curve.
    Element(SetSelection),
    /// At a point on the chain.
    Point(Construction<kinds::Point>),
}

/// How the listed sets of a fillet or a chamfer meet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// The indices of the sets that meet.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<u32>"))]
    pub between: BTreeSet<u32>,
    /// The vertices or edges at which they meet.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_at"
    )]
    pub at: Option<SetSelection>,
    /// How they meet.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transition_kind"
    )]
    pub kind: Option<TransitionKind>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
}

/// How the listed sets of a transition meet.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TransitionKind {
    /// Carries the round through the junction as one rolling round and adds
    /// no surface of another kind: where the two listed pieces' ends do not
    /// coincide, the round's own fillet face at the pieces' ball spans
    /// between them, tangent to both.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Continue,
    /// Extends overlapping pieces toward each other until they merge in a
    /// sharp boundary.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Intersect,
    /// Ends the round where its edge chain ends: the last piece is cut by
    /// the carriers of the faces that meet both rounded faces at the chain's
    /// end, a face whose carrier must be continued to close the end gains a
    /// face on that carrier, and no surface of another kind is added.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Stop,
    /// Ends the round at a datum point or plane.
    StopAt(LimitAt),
    /// Closes three overlapping pieces with a sphere at the transition
    /// distances along the edges, blended into the pieces by fillet
    /// surfaces.
    CornerSphere {
        /// The sphere's radius. `None` is the largest piece's radius.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
        /// The transition distances along the edges.
        lengths: Vec<Dimension<NonNegativeLength>>,
    },
    /// A ball of radius r stays tangent to the listed pieces and to the faces
    /// in `against`, inside every listed piece and outside only a piece in
    /// `against`, its centre at ρ − r from the centre locus of a piece it
    /// lies inside and ρ + r from one it lies outside (a cylinder's axis, a
    /// torus's centre circle, a fillet's ball-centre curve): a sphere where
    /// all radii are r; a torus about the R piece's axis, tube r, centre
    /// circle R − r, for r, r, R; and for r, R₁, R₂ the sweep of the ball
    /// whose centre stays at R₁ − r and R₂ − r from the two larger pieces'
    /// loci (R₁ = R₂ included), with, where the sweep reaches the face the
    /// pieces roll on, the ball rolling on the larger piece and on that face.
    RollingBall {
        /// r. `None` is the smallest radius among the listed pieces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
        /// The faces, of the state entering the round and of no listed set,
        /// that the ball stays tangent to along the corner face. `None`
        /// states that it has no such support.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_against"
        )]
        against: Option<SetSelection>,
    },
    /// The round of radius r swept outside one listed piece, the wrapped
    /// one, its centre at ρ + r from that piece's centre locus, tangent to
    /// `against`, where the rounds the corner continues have radius r. With
    /// three listed sets the wrapped piece is the largest and the sweep is
    /// the torus about its axis, tube r and centre circle R + r. With two,
    /// the corner continues the other listed set's round. With one, that set
    /// is wrapped by another feature's round of radius r. For a cylinder
    /// against a plane normal to its axis the sweep is the torus of tube r
    /// and centre circle R + r.
    SweptAround {
        /// r. `None` is the smallest radius among the listed pieces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
        /// The faces, of the state entering the round and of no listed set,
        /// that the ball stays tangent to along the corner face. `None`
        /// states that it has no such support.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_against"
        )]
        against: Option<SetSelection>,
    },
    /// Two chain ends joined by the round continued across the corner:
    /// fillet faces at the pieces' radius, joined to both end pieces across
    /// the round's seam, rolling on the face both chains roll on and on the
    /// corner's faces between them; where they meet, the junction's fillets
    /// and each end piece open at the same angle within 0.1 rad.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Blend,
    /// A transition, under its source name, whose shape or sizes are not
    /// established.
    Native(NonBlankString),
}

/// What a blend does where it runs past a neighbouring edge.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Overflow {
    /// The source's default.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Default,
    /// The blend keeps the neighbouring edge.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    KeepEdge,
    /// The blend keeps the neighbouring surface.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    KeepSurface,
    /// A source overflow whose meaning is not established.
    Native(NonBlankString),
}

/// Hollows bodies to a wall, opening it at the removed faces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShellFeature {
    /// The bodies to shell, where the source names them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bodies"
    )]
    pub bodies: Option<SetSelection>,
    /// The faces that open the shell. A `Named` selection with no picks
    /// states a closed hollow.
    pub removed: SetSelection,
    /// The wall thickness. A `OneSide` thickness `Inside` grows the wall into
    /// the material and `Outside` away from it, `Offsets` holds the outside
    /// and inside values, and `Symmetric` splits the value across the
    /// boundary.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<Thickness>,
    /// Face sets with their own thickness on the default side.
    pub faces: Vec<FaceThickness>,
    /// The corner law between offset faces.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_join"
    )]
    pub join: Option<OffsetJoin>,
    /// Which offset faces trim each other where their offsets meet.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_face_meeting"
    )]
    pub face_meeting: Option<FaceMeeting>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A face set's own shell thickness, on the default side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FaceThickness {
    /// The faces.
    pub faces: SetSelection,
    /// Their thickness.
    pub thickness: Dimension<PositiveLength>,
}

/// Which offset faces trim each other where their offsets meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FaceMeeting {
    /// Each offset face is intersected with its neighbours only. This gives
    /// the result of `All` wherever no faces that are not adjacent meet, and
    /// no valid result where they do.
    Neighbours,
    /// Every offset face is intersected with every other, so offset faces
    /// that were not adjacent and meet after the offset trim each other and
    /// the result is the offset solid.
    All,
}

/// Thickens faces into a solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Thicken {
    /// The faces.
    pub faces: SetSelection,
    /// The thickness. `Outside` is along the face normal and `Inside`
    /// against it; `Offsets` holds both offsets.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<Thickness>,
    /// How the thickened body combines with existing bodies.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boolean"
    )]
    pub boolean: Option<Boolean>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// Moves faces by a signed distance along their oriented normals, per set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Offset {
    /// The face sets, each with its distance.
    pub sets: Vec<OffsetSet>,
    /// Whether the faces move in place or a new body is made.
    pub result: OffsetResult,
    /// The corner law between offset faces.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_join"
    )]
    pub join: Option<OffsetJoin>,
    /// Which offset faces trim each other where their offsets meet.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_face_meeting"
    )]
    pub face_meeting: Option<FaceMeeting>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// One face set of an offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OffsetSet {
    /// The faces.
    pub faces: SetSelection,
    /// The signed distance along the faces' oriented normals. A source's
    /// flip of the offset is the distance's sign.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_distance"
    )]
    pub distance: Option<Dimension<Length>>,
}

/// What an offset makes.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OffsetResult {
    /// Changes the faces in place.
    Move {},
    /// Makes a new body with its output: a sheet, or a solid where the copy
    /// closes.
    Copy(SheetOutput),
}

/// Tilts the faces of each set by its angle relative to the pull direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Draft {
    /// The sets, each with its angle.
    pub sets: Vec<DraftSet>,
    /// The pull direction. Where the source derives it from the neutral
    /// plane, the construction holds one `Whole` `Perpendicular` input on
    /// that plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pull"
    )]
    pub pull: Option<Construction<kinds::Direction>>,
    /// The fixed plane: a datum, a planar face, or an origin plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_neutral"
    )]
    pub neutral: Option<Construction<kinds::Plane>>,
    /// The element along which faces split and draft away on each side:
    /// edges, curves, a surface, or a plane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parting"
    )]
    pub parting: Option<SetSelection>,
    /// Makes a parting-line draft a step draft: each drafted face is one
    /// face rotated about the plane the pull direction is normal to, leaving
    /// small step faces between the parting line's segments.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_steps"
    )]
    pub steps: Option<StepFaces>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// One face set of a draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DraftSet {
    /// The drafted faces. `None` where the source drafts the faces adjacent
    /// to the `fixed` edges without stating them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_faces"
    )]
    pub faces: Option<SetSelection>,
    /// The edges of the set that stay fixed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fixed"
    )]
    pub fixed: Option<SetSelection>,
    /// The angle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft_angle"
    )]
    pub angle: Option<DraftAngle>,
}

/// The angle of a draft set.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DraftAngle {
    /// One angle.
    One(Dimension<SlopeAngle>),
    /// The faces on the side of the neutral or parting element that the pull
    /// direction points to by `first`, and on the other side by `second`.
    Two {
        /// The angle on the side the pull direction points to.
        first: Dimension<SlopeAngle>,
        /// The angle on the other side.
        second: Dimension<SlopeAngle>,
    },
    /// One angle on both sides.
    Symmetric(Dimension<SlopeAngle>),
    /// The angle varying along the fixed chain.
    Variable(Stations<SlopeAngle>),
    /// A source angle form whose meaning is not established.
    Native(NonBlankString),
}

/// The step faces of a step draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepFaces {
    /// Steps generated as the tapered faces are.
    Tapered,
    /// Steps perpendicular to the original face.
    Perpendicular,
}

/// Drills one shape at every position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Hole {
    /// The positions.
    pub positions: HolePositions,
    /// The face the holes start on.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start"
    )]
    pub start: Option<SetSelection>,
    /// The drilling direction, normal to `start` unless a reference says
    /// otherwise. A source's flip is a `Flip` step of the construction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<Construction<kinds::Direction>>,
    /// The shape.
    pub shape: HoleShape,
    /// The one-sided limit. The depth runs from the start face to the
    /// shoulder unless `DepthTo::Tip` says otherwise.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extent"
    )]
    pub extent: Option<Limit<Length>>,
    /// The standard that sizes a clearance or drilled hole.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_standard"
    )]
    pub standard: Option<StandardSize>,
    /// The hole's internal thread. A tapped hole's designation is its
    /// standard.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread"
    )]
    pub thread: Option<ThreadSpec>,
    /// The bodies the hole cuts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_targets"
    )]
    pub targets: Option<Targets>,
    /// The stages of a hole run through several parts of an assembly. The
    /// hole's own shape, extent, standard, and thread cut every participant
    /// the axis passes through that no stage picks. An empty series is a
    /// single hole.
    pub series: Vec<SeriesStage>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// One stage of a hole series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SeriesStage {
    /// The parts the stage names, each a participant of the assembly
    /// feature.
    pub parts: SetSelection,
    /// The stage's hole.
    pub hole: StageHole,
}

/// The hole of one series stage.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StageHole {
    /// The stage's own hole, which the assembly feature cuts in each picked
    /// part.
    Stated {
        /// The shape.
        shape: Box<HoleShape>,
        /// The limit of the cut in each part the stage picks. An `Amount`
        /// runs from the first face the stage's axis meets in that part.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_stage_extent"
        )]
        extent: Option<Box<Limit<Length>>>,
        /// The standard that sizes the hole.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_standard"
        )]
        standard: Option<StandardSize>,
        /// The internal thread.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_stage_thread"
        )]
        thread: Option<Box<ThreadSpec>>,
    },
    /// The stage is a hole feature of the picked part's own history, placed
    /// by the series, which cuts nothing itself.
    PartFeature(FeatureId),
}

/// Where a hole feature drills.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HolePositions {
    /// Picked sketch points, vertices, or datum points, or a whole sketch
    /// whose points are positions.
    Points {
        /// The picks.
        points: OrderedSelection,
        /// Which geometry of a whole-sketch pick yields positions.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_centers"
        )]
        centers: Option<BTreeSet<CenterSource>>,
    },
    /// One construction per position that relates it to references with
    /// dimensions. A construction states no `Coincident` input for the start
    /// face.
    Placed(Vec<Construction<kinds::Point>>),
}

/// Geometry of a whole-sketch pick that yields hole positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CenterSource {
    /// Sketch points.
    Points,
    /// The centres of circles.
    Circles,
    /// The centres of arcs.
    Arcs,
}

/// The shape a hole drills.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoleShape {
    /// A bore, from entry to bottom.
    Bore(Bore),
    /// A sketched hole: the profile revolves about the hole axis.
    Revolved(ProfileRef),
    /// The hole's section, its head included, swept along a segment centred
    /// on its position point in `direction`, so its end arcs have the bore's
    /// radius and a head's arcs are concentric with them.
    Slot {
        /// The section.
        bore: Bore,
        /// The slot's length, read by `measure`.
        length: Dimension<PositiveLength>,
        /// How `length` is read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_slot_measure"
        )]
        measure: Option<SlotLength>,
        /// The direction from the position point toward the first end point.
        direction: Box<Construction<kinds::Direction>>,
    },
}

/// How a slot's length is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SlotLength {
    /// The end-arc centres lie `length` apart.
    CenterToCenter,
    /// The end-arc centres lie `length` less the bore diameter apart.
    TangentToTangent,
}

/// A hole's bore, from entry to bottom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Bore {
    /// The bore diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_diameter"
    )]
    pub diameter: Option<Dimension<PositiveLength>>,
    /// The entry recess.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_head"
    )]
    pub head: Option<HoleHead>,
    /// The taper that makes the bore conical.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_taper"
    )]
    pub taper: Option<Dimension<SlopeAngle>>,
    /// The bottom.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bottom"
    )]
    pub bottom: Option<HoleBottom>,
    /// A countersink at the entry, above the head.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_near_chamfer"
    )]
    pub near_chamfer: Option<Cone>,
    /// A countersink between the head and the bore.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_neck_chamfer"
    )]
    pub neck_chamfer: Option<Cone>,
    /// A countersink at the exit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_far_chamfer"
    )]
    pub far_chamfer: Option<Cone>,
}

/// The entry recess of a hole.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HoleHead {
    /// No recess.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Plain,
    /// A cylindrical recess. The hole's extent and its thread depth are
    /// measured from the start face.
    Counterbore {
        /// The recess diameter.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_diameter"
        )]
        diameter: Option<Dimension<PositiveLength>>,
        /// The recess depth.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_depth"
        )]
        depth: Option<Dimension<PositiveLength>>,
    },
    /// A conical recess.
    Countersink(Cone),
    /// A cylindrical recess entered through a cone of included `angle`.
    Counterdrill {
        /// The recess diameter.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_diameter"
        )]
        diameter: Option<Dimension<PositiveLength>>,
        /// The recess depth.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_depth"
        )]
        depth: Option<Dimension<PositiveLength>>,
        /// The included angle of the entry cone.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<InteriorAngle>>,
    },
    /// A recess of `diameter` and `depth`, like a counterbore's, from whose
    /// floor the hole's extent and its thread depth are measured.
    Spotface {
        /// The recess diameter.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_diameter"
        )]
        diameter: Option<Dimension<PositiveLength>>,
        /// The recess depth.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_spotface_depth"
        )]
        depth: Option<Dimension<NonNegativeLength>>,
    },
    /// A source recess whose meaning is not established.
    Native(NonBlankString),
}

/// A conical recess or chamfer of a hole. It holds the two of diameter,
/// depth, and included angle that the source drives; the third is absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Cone {
    /// The diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_diameter"
    )]
    pub diameter: Option<Dimension<PositiveLength>>,
    /// The depth.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_depth"
    )]
    pub depth: Option<Dimension<PositiveLength>>,
    /// The included angle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<InteriorAngle>>,
}

/// The bottom of a bore.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HoleBottom {
    /// A flat bottom.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Flat,
    /// A conical drill point.
    DrillPoint {
        /// The point's included angle.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<InteriorAngle>>,
        /// Where the hole's depth runs to.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_depth_to"
        )]
        depth_to: Option<DepthTo>,
    },
    /// Ends the bore on the face that bounds its extent, which forms the
    /// bottom, with no cone or cap of its own inside the material.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Trimmed,
    /// A source bottom whose meaning is not established.
    Native(NonBlankString),
}

/// Where a hole's depth runs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DepthTo {
    /// To the shoulder.
    Shoulder,
    /// To the drill point's tip.
    Tip,
}

/// A size from a standard: standard, fastener kind, size, and fit or
/// tolerance class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StandardSize {
    /// The standard.
    pub standard: NonBlankString,
    /// The fastener kind.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_fastener_kind"
    )]
    pub kind: Option<NonBlankString>,
    /// The size designation. A size bound by expression binds a `Text`
    /// parameter holding the designation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_designation"
    )]
    pub size: Option<Dimension<NonBlankString>>,
    /// The fit class, or for a thread the tolerance class.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_class"
    )]
    pub class: Option<NonBlankString>,
}

/// Threads a cylindrical face that no hole owns: a shaft, a boss, or a bore
/// made by another feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Thread {
    /// The cylindrical face.
    pub face: SetSelection,
    /// The edge or face the thread starts from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_start"
    )]
    pub start: Option<SetSelection>,
    /// The thread.
    pub spec: ThreadSpec,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A thread's form and extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ThreadSpec {
    /// The standard, the size designation, and the tolerance class.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_standard"
    )]
    pub standard: Option<StandardSize>,
    /// The major diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_major_diameter"
    )]
    pub major_diameter: Option<Dimension<PositiveLength>>,
    /// The minor diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_minor_diameter"
    )]
    pub minor_diameter: Option<Dimension<PositiveLength>>,
    /// The pitch diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pitch_diameter"
    )]
    pub pitch_diameter: Option<Dimension<PositiveLength>>,
    /// The pitch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pitch"
    )]
    pub pitch: Option<Dimension<PositiveLength>>,
    /// The turning sense.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hand"
    )]
    pub hand: Option<Hand>,
    /// The thread's length.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_length"
    )]
    pub length: Option<ThreadLength>,
    /// Whether the body holds the helical form, or carries the thread as
    /// data.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_modeled"
    )]
    pub modeled: Option<Dimension<bool>>,
}

/// How far a thread runs.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ThreadLength {
    /// Over the whole face or hole depth.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Full,
    /// A stated length.
    Blind(Dimension<PositiveLength>),
    /// To a limit face or plane.
    ToElement {
        /// The limit face or plane.
        element: SetSelection,
        /// The offset from the element.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
    /// The length from the standard's tapped-hole rule.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Standard,
    /// A source length whose meaning is not established.
    Native(NonBlankString),
}

/// Raises or sinks a profile, often text, by a depth, or imprints it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Emboss {
    /// The profile.
    pub profile: ProfileRef,
    /// Whether the profile is raised, sunk, or imprinted.
    pub effect: EmbossEffect,
    /// The face the profile maps onto.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_face"
    )]
    pub face: Option<SetSelection>,
    /// How the profile maps onto the face.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mapping"
    )]
    pub mapping: Option<EmbossMapping>,
    /// The direction a projection runs along.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<Construction<kinds::Direction>>,
    /// The bodies the emboss joins or cuts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_targets"
    )]
    pub targets: Option<Targets>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// What an emboss does with its profile.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EmbossEffect {
    /// Raises the profile.
    Emboss {
        /// The height it is raised by.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_depth"
        )]
        depth: Option<Dimension<PositiveLength>>,
    },
    /// Sinks the profile.
    Engrave {
        /// The depth it is sunk by.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_depth"
        )]
        depth: Option<Dimension<PositiveLength>>,
    },
    /// Imprints the profile.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Scribe,
    /// A source effect whose meaning is not established.
    Native(NonBlankString),
}

/// How an emboss maps its profile onto the face.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EmbossMapping {
    /// Maps the profile onto the face by arc length.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Wrap,
    /// Projects the profile along the direction.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Project,
    /// A source mapping whose meaning is not established.
    Native(NonBlankString),
}

/// Raises each face to a smooth cap that meets the face's outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Dome {
    /// The faces.
    pub faces: SetSelection,
    /// The height of the cap's highest point above the face's plane, along
    /// the face normal, or along `direction` where it is stated. `None` where
    /// `points` constrains the cap. On a cylindrical or conical model a
    /// height of 0 makes the cap tangent to the adjacent face.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height"
    )]
    pub height: Option<Dimension<NonNegativeLength>>,
    /// Whether the cap is a recess.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_concave"
    )]
    pub concave: Option<Dimension<bool>>,
    /// The cap's form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_dome_shape"
    )]
    pub shape: Option<DomeShape>,
    /// The direction the height is measured along.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<Construction<kinds::Direction>>,
    /// Points or a sketch that constrain the cap.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_points"
    )]
    pub points: Option<SetSelection>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The form of a dome's cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DomeShape {
    /// The cap rises normal to a polygonal face's edges.
    Default,
    /// A half ellipsoid whose height is one of its radii, on a cylindrical or
    /// conical model.
    Elliptical,
    /// The cap slopes up evenly on all sides of a polygonal face.
    Continuous,
}

/// Binds a raster image, an asset of the model, to model faces as an
/// appearance. It changes no geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Decal {
    /// The image.
    pub image: AssetId,
    /// The faces. The first positions and orients the image.
    pub faces: OrderedSelection,
    /// Whether the image wraps onto the faces connected to the first.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chain_faces"
    )]
    pub chain_faces: Option<Dimension<bool>>,
    /// The opacity, from 0, transparent, to 1, opaque.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opacity"
    )]
    pub opacity: Option<Dimension<Fraction>>,
    /// Where the image lies on the faces. A decal whose image lies on its
    /// faces another way states none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    pub placement: Option<DecalPlacement>,
    /// Source options whose meaning is not established, and the values of
    /// typed facts whose carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// Where a decal's image lies on its faces. The image point at distance r
/// from the image's centre, in the direction at angle θ from the width axis
/// toward the height axis, shows at the point of the faces at surface
/// distance r from the origin along the geodesic that leaves the origin at
/// angle θ from x toward y, at the one scale `width` and `height` state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DecalPlacement {
    /// The image frame: its origin at the image's centre on the faces, its x
    /// axis along the image's width, from left to right, its y axis along
    /// its height, from bottom to top, and its z axis, x × y, along the
    /// faces' outward normal at the origin.
    pub frame: Construction<kinds::Frame>,
    /// The image's width on the faces.
    pub width: Dimension<PositiveLength>,
    /// The image's height on the faces.
    pub height: Dimension<PositiveLength>,
    /// Whether the image is reflected across its height axis, so that its
    /// width runs against x.
    pub mirrored: bool,
}

/// Reads a stated `centers` set, refusing `null`, a repeated member, and
/// members out of order.
fn deserialize_centers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeSet<CenterSource>>, D::Error> {
    deserialize_center_set(deserializer).map(|centers| centers.map(|CenterSet(set)| set))
}

/// A set of center sources, read as a §2.5 set.
#[derive(Deserialize)]
#[serde(transparent)]
struct CenterSet(#[serde(with = "cadmpeg_core::distinct_keys::set")] BTreeSet<CenterSource>);

cadmpeg_core::named_optional_field!(deserialize_center_set, CenterSet, "centers");

cadmpeg_core::named_optional_field!(deserialize_against, SetSelection, "against");
cadmpeg_core::named_optional_field!(deserialize_angle, Dimension<InteriorAngle>, "angle");
cadmpeg_core::named_optional_field!(deserialize_at, SetSelection, "at");
cadmpeg_core::named_optional_field!(deserialize_bodies, SetSelection, "bodies");
cadmpeg_core::named_optional_field!(deserialize_boolean, Boolean, "boolean");
cadmpeg_core::named_optional_field!(deserialize_bottom, HoleBottom, "bottom");
cadmpeg_core::named_optional_field!(deserialize_chain_faces, Dimension<bool>, "chain_faces");
cadmpeg_core::named_optional_field!(deserialize_chain_start, SetSelection, "chain_start");
cadmpeg_core::named_optional_field!(deserialize_chamfer_measure, ChamferMeasure, "measure");
cadmpeg_core::named_optional_field!(deserialize_chamfer_size, ChamferSize, "size");
cadmpeg_core::named_optional_field!(deserialize_class, NonBlankString, "class");
cadmpeg_core::named_optional_field!(deserialize_concave, Dimension<bool>, "concave");
cadmpeg_core::named_optional_field!(deserialize_corner_style, CornerStyle, "corner_style");
cadmpeg_core::named_optional_field!(deserialize_depth, Dimension<PositiveLength>, "depth");
cadmpeg_core::named_optional_field!(deserialize_depth_to, DepthTo, "depth_to");
cadmpeg_core::named_optional_field!(deserialize_designation, Dimension<NonBlankString>, "size");
cadmpeg_core::named_optional_field!(deserialize_diameter, Dimension<PositiveLength>, "diameter");
cadmpeg_core::named_optional_field!(
    deserialize_direction,
    Construction<kinds::Direction>,
    "direction"
);
cadmpeg_core::named_optional_field!(deserialize_distance, Dimension<Length>, "distance");
cadmpeg_core::named_optional_field!(deserialize_dome_shape, DomeShape, "shape");
cadmpeg_core::named_optional_field!(deserialize_draft_angle, DraftAngle, "angle");
cadmpeg_core::named_optional_field!(deserialize_extent, Limit<Length>, "extent");
cadmpeg_core::named_optional_field!(deserialize_face, SetSelection, "face");
cadmpeg_core::named_optional_field!(deserialize_face_meeting, FaceMeeting, "face_meeting");
cadmpeg_core::named_optional_field!(deserialize_faces, SetSelection, "faces");
cadmpeg_core::named_optional_field!(deserialize_far_chamfer, Cone, "far_chamfer");
cadmpeg_core::named_optional_field!(deserialize_fastener_kind, NonBlankString, "kind");
cadmpeg_core::named_optional_field!(deserialize_fillet_size, FilletSize, "size");
cadmpeg_core::named_optional_field!(deserialize_fixed, SetSelection, "fixed");
cadmpeg_core::named_optional_field!(deserialize_flip, Dimension<bool>, "flip");
cadmpeg_core::named_optional_field!(deserialize_hand, Hand, "hand");
cadmpeg_core::named_optional_field!(deserialize_head, HoleHead, "head");
cadmpeg_core::named_optional_field!(deserialize_height, Dimension<NonNegativeLength>, "height");
cadmpeg_core::named_optional_field!(deserialize_hold, SetSelection, "hold");
cadmpeg_core::named_optional_field!(deserialize_join, OffsetJoin, "join");
cadmpeg_core::named_optional_field!(deserialize_keep, SetSelection, "keep");
cadmpeg_core::named_optional_field!(deserialize_keep_features, Dimension<bool>, "keep_features");
cadmpeg_core::named_optional_field!(deserialize_law, Interpolation, "law");
cadmpeg_core::named_optional_field!(deserialize_length, ThreadLength, "length");
cadmpeg_core::named_optional_field!(
    deserialize_major_diameter,
    Dimension<PositiveLength>,
    "major_diameter"
);
cadmpeg_core::named_optional_field!(deserialize_mapping, EmbossMapping, "mapping");
cadmpeg_core::named_optional_field!(
    deserialize_minor_diameter,
    Dimension<PositiveLength>,
    "minor_diameter"
);
cadmpeg_core::named_optional_field!(deserialize_modeled, Dimension<bool>, "modeled");
cadmpeg_core::named_optional_field!(deserialize_near, Construction<kinds::Point>, "near");
cadmpeg_core::named_optional_field!(deserialize_near_chamfer, Cone, "near_chamfer");
cadmpeg_core::named_optional_field!(deserialize_neck_chamfer, Cone, "neck_chamfer");
cadmpeg_core::named_optional_field!(deserialize_neutral, Construction<kinds::Plane>, "neutral");
cadmpeg_core::named_optional_field!(deserialize_offset, Dimension<Length>, "offset");
cadmpeg_core::named_optional_field!(deserialize_opacity, Dimension<Fraction>, "opacity");
cadmpeg_core::named_optional_field!(deserialize_overflow, Overflow, "overflow");
cadmpeg_core::named_optional_field!(deserialize_parting, SetSelection, "parting");
cadmpeg_core::named_optional_field!(deserialize_pitch, Dimension<PositiveLength>, "pitch");
cadmpeg_core::named_optional_field!(
    deserialize_pitch_diameter,
    Dimension<PositiveLength>,
    "pitch_diameter"
);
cadmpeg_core::named_optional_field!(deserialize_placement, DecalPlacement, "placement");
cadmpeg_core::named_optional_field!(deserialize_points, SetSelection, "points");
cadmpeg_core::named_optional_field!(deserialize_profile, FilletProfile, "profile");
cadmpeg_core::named_optional_field!(deserialize_pull, Construction<kinds::Direction>, "pull");
cadmpeg_core::named_optional_field!(deserialize_radius, Dimension<PositiveLength>, "radius");
cadmpeg_core::named_optional_field!(deserialize_reference, SetSelection, "reference");
cadmpeg_core::named_optional_field!(deserialize_reversed, Dimension<bool>, "reversed");
cadmpeg_core::named_optional_field!(deserialize_rho, Dimension<Fraction>, "rho");
cadmpeg_core::named_optional_field!(deserialize_round_corners, Dimension<bool>, "round_corners");
cadmpeg_core::named_optional_field!(deserialize_setbacks, Setbacks, "setbacks");
cadmpeg_core::named_optional_field!(deserialize_shape_factor, Dimension<Fraction>, "shape");
cadmpeg_core::named_optional_field!(deserialize_slot_measure, SlotLength, "measure");
cadmpeg_core::named_optional_field!(deserialize_spine, PathRef, "spine");
cadmpeg_core::named_optional_field!(
    deserialize_spotface_depth,
    Dimension<NonNegativeLength>,
    "depth"
);
cadmpeg_core::named_optional_field!(deserialize_stage_extent, Box<Limit<Length>>, "extent");
cadmpeg_core::named_optional_field!(deserialize_stage_thread, Box<ThreadSpec>, "thread");
cadmpeg_core::named_optional_field!(deserialize_standard, StandardSize, "standard");
cadmpeg_core::named_optional_field!(deserialize_start, SetSelection, "start");
cadmpeg_core::named_optional_field!(deserialize_steps, StepFaces, "steps");
cadmpeg_core::named_optional_field!(deserialize_taper, Dimension<SlopeAngle>, "taper");
cadmpeg_core::named_optional_field!(deserialize_targets, Targets, "targets");
cadmpeg_core::named_optional_field!(deserialize_thickness, Thickness, "thickness");
cadmpeg_core::named_optional_field!(deserialize_thread, ThreadSpec, "thread");
cadmpeg_core::named_optional_field!(deserialize_transition_kind, TransitionKind, "kind");
cadmpeg_core::named_optional_field!(deserialize_trim, Dimension<bool>, "trim");
cadmpeg_core::named_optional_field!(deserialize_weight, Dimension<PositiveReal>, "weight");
