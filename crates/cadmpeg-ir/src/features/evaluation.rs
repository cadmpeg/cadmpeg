// SPDX-License-Identifier: Apache-2.0
//! Feature evaluations and their input and result topology.

use super::geometry::FinitePoint3;
use super::membership;
use super::selections::{BodySelectionError, DistinctMembers, NonEmptyMembers};
use super::{FeatureDefinition, FeatureId};
use crate::ids::{
    BodyId, EdgeId, FaceId, FeatureInputTopologyId, FeatureResultTopologyId, HistoricalBodyId,
    HistoricalEdgeId, HistoricalFaceId, HistoricalVertexId, HistoryBodyId, LoopId,
};
use crate::units::UnitVector3;
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_edge_geometry_carrier,
    HistoricalEdgeCarrier,
    "carrier"
);

cadmpeg_core::named_optional_field!(deserialize_edge_geometry_end, FinitePoint3, "end");

cadmpeg_core::named_optional_field!(deserialize_edge_geometry_start, FinitePoint3, "start");

cadmpeg_core::named_optional_field!(deserialize_edge_geometry_tangent, UnitVector3, "tangent");

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

/// A feature definition and the bodies its evaluation produces.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEvaluation {
    pub(super) definition: FeatureDefinition,
    pub(super) outputs: DistinctMembers<BodyId>,
}

impl FeatureEvaluation {
    /// Construct an evaluation from its semantics and produced bodies.
    #[must_use]
    pub const fn new(definition: FeatureDefinition, outputs: DistinctMembers<BodyId>) -> Self {
        Self {
            definition,
            outputs,
        }
    }

    /// Construct an evaluation with no produced bodies.
    #[must_use]
    pub const fn from_definition(definition: FeatureDefinition) -> Self {
        Self {
            definition,
            outputs: DistinctMembers(Vec::new()),
        }
    }

    /// Consume the evaluation and return its semantics and produced bodies.
    pub fn into_parts(self) -> (FeatureDefinition, DistinctMembers<BodyId>) {
        (self.definition, self.outputs)
    }

    /// Return the neutral construction semantics.
    pub const fn definition(&self) -> &FeatureDefinition {
        &self.definition
    }

    /// Return the produced or modified body identities.
    pub const fn outputs(&self) -> &Vec<BodyId> {
        &self.outputs.0
    }

    /// Edit the semantics and the produced bodies together.
    pub fn edit(
        &mut self,
        edit: impl FnOnce(&mut FeatureDefinition, &mut DistinctMembers<BodyId>),
    ) {
        edit(&mut self.definition, &mut self.outputs);
    }

    /// Replace the construction semantics.
    pub fn set_definition(&mut self, definition: FeatureDefinition) {
        self.definition = definition;
    }

    /// Replace the produced body identities.
    pub fn set_outputs(&mut self, outputs: DistinctMembers<BodyId>) {
        self.outputs = outputs;
    }
}

/// Typed topology membership at one feature's evaluation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeatureInputTopology {
    /// Globally unique state id.
    pub id: FeatureInputTopologyId,
    /// Feature evaluated from this state.
    pub input_of: FeatureId,
    /// Bodies present in this state.
    pub bodies: DistinctMembers<HistoricalBodyId>,
    /// Faces present in this state.
    pub faces: DistinctMembers<HistoricalFaceId>,
    /// Edges present in this state.
    pub edges: DistinctMembers<HistoricalEdgeId>,
    /// Vertices present in this state.
    pub vertices: DistinctMembers<HistoricalVertexId>,
    /// Sampled geometry of edges present in this state, at most one entry
    /// per edge.
    pub edge_geometry: Vec<HistoricalEdgeGeometry>,
    /// The body of this state that holds each of some of its faces, at most
    /// one entry per face. A face with no entry has an owner this state does
    /// not state.
    pub face_owners: Vec<HistoricalFaceOwner>,
    /// Carrier surfaces of faces present in this state, at most one entry per
    /// face.
    pub face_geometry: Vec<HistoricalFaceGeometry>,
    /// The feature that last writes each of some of the bodies of this state,
    /// at most one entry per body.
    pub body_writers: Vec<HistoricalBodyWriter>,
    /// Full-fidelity source state reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The feature whose result last inserts or updates one body of a feature's
/// input state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalBodyWriter {
    /// Body of the owning input state.
    pub body: HistoricalBodyId,
    /// Feature whose result last inserts or updates the body before the state.
    pub writer: FeatureId,
}

/// The body of a feature's input state that holds one face of that state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalFaceOwner {
    /// Face of the owning input state.
    pub face: HistoricalFaceId,
    /// Body of the owning input state whose shells hold the face.
    pub body: HistoricalBodyId,
}

/// One face beside a historical edge that carries an asymmetric chamfer distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EdgeSupport {
    /// Face of the owning input state.
    pub face: HistoricalFaceId,
    /// Outward unit normal of the face at the edge midpoint.
    pub normal: UnitVector3,
    /// The face's coedge runs with the edge, so the face lies on the left.
    pub with_edge: bool,
}

/// Geometry of one historical edge in a feature's input state.
///
/// Positions and directions are in millimetres in the frame of the body that
/// the feature evaluates, before the feature applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalEdgeGeometry {
    /// Edge of the owning input state.
    pub edge: HistoricalEdgeId,
    /// Exact topology in the consumed history-body nest.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_topology"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "HistoricalEdgeTopologyRef"))]
    pub topology: Option<HistoricalEdgeTopologyRef>,
    /// Point on the edge at the midpoint of its carrier parameter interval.
    pub interior: FinitePoint3,
    /// Unit carrier tangent at `interior`, in the carrier's parameter
    /// direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_tangent"
    )]
    pub tangent: Option<UnitVector3>,
    /// Position of the start vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_start"
    )]
    pub start: Option<FinitePoint3>,
    /// Position of the end vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_end"
    )]
    pub end: Option<FinitePoint3>,
    /// Faces of the owning input state that use the edge.
    pub faces: DistinctMembers<HistoricalFaceId>,
    /// Carrier curve of the edge and the parameter interval the edge covers
    /// on it, in the frame of `interior`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_geometry_carrier"
    )]
    pub carrier: Option<HistoricalEdgeCarrier>,
    /// Support faces of an unequal two-distance chamfer that selects this edge.
    /// Empty for every other edge.
    pub supports: Vec<EdgeSupport>,
}

/// Carrier of one historical edge: a curve that holds no arena references,
/// and the parameter interval of the edge on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalEdgeCarrier {
    /// Carrier curve.
    pub curve: HistoricalEdgeCurve,
    /// Parameter interval of the edge on `curve`, lower endpoint first.
    pub range: crate::topology::ParameterInterval,
}

/// Shape of a historical edge's carrier curve.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HistoricalEdgeCurve {
    /// Infinite line.
    Line(crate::geometry::analytic::LineCurve),
    /// Full circle.
    Circle(crate::geometry::analytic::CircleCurve),
    /// Ellipse.
    Ellipse(crate::geometry::analytic::EllipseCurve),
    /// Parabola in STEP conic form.
    Parabola(crate::geometry::analytic::ParabolaCurve),
    /// Hyperbola in STEP conic form.
    Hyperbola(crate::geometry::analytic::HyperbolaCurve),
    /// Free-form NURBS curve.
    Nurbs(crate::geometry::nurbs::NurbsCurve),
}

impl HistoricalEdgeCurve {
    /// The carrier of `geometry`, absent for a carrier that refers to other
    /// curves or holds no closed-form shape.
    #[must_use]
    pub fn from_solved(geometry: &crate::geometry::SolvedCurveGeometry) -> Option<Self> {
        use crate::geometry::SolvedCurveGeometry as Solved;
        match geometry {
            Solved::Line(curve) => Some(Self::Line(*curve)),
            Solved::Circle(curve) => Some(Self::Circle(*curve)),
            Solved::Ellipse(curve) => Some(Self::Ellipse(*curve)),
            Solved::Parabola(curve) => Some(Self::Parabola(*curve)),
            Solved::Hyperbola(curve) => Some(Self::Hyperbola(*curve)),
            Solved::Nurbs(curve) => Some(Self::Nurbs(curve.clone())),
            Solved::Degenerate(_)
            | Solved::Composite { .. }
            | Solved::Polyline(_)
            | Solved::Transformed(_)
            | Solved::Unknown { .. } => None,
        }
    }

    /// The carrier as solved curve geometry, for evaluation.
    #[must_use]
    pub fn to_solved(&self) -> crate::geometry::SolvedCurveGeometry {
        use crate::geometry::SolvedCurveGeometry as Solved;
        match self {
            Self::Line(curve) => Solved::Line(*curve),
            Self::Circle(curve) => Solved::Circle(*curve),
            Self::Ellipse(curve) => Solved::Ellipse(*curve),
            Self::Parabola(curve) => Solved::Parabola(*curve),
            Self::Hyperbola(curve) => Solved::Hyperbola(*curve),
            Self::Nurbs(curve) => Solved::Nurbs(curve.clone()),
        }
    }
}

/// Carrier surface of one historical face in a feature's input state.
///
/// The surface is in millimetres in the frame of the body that the feature
/// evaluates, before the feature applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalFaceGeometry {
    /// Face of the owning input state.
    pub face: HistoricalFaceId,
    /// Carrier surface of the face.
    pub surface: HistoricalFaceSurface,
    /// The face normal is opposite to the natural normal of `surface`.
    pub reversed: bool,
}

/// Shape of a historical face's carrier surface.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HistoricalFaceSurface {
    /// Infinite plane.
    Plane(crate::geometry::analytic::PlaneSurface),
    /// Right circular cylinder.
    Cylinder(crate::geometry::analytic::CylinderSurface),
    /// Right elliptical cone.
    Cone(crate::geometry::analytic::ConeSurface),
    /// Sphere.
    Sphere(crate::geometry::analytic::SphereSurface),
    /// Torus.
    Torus(crate::geometry::analytic::TorusSurface),
    /// Free-form NURBS surface.
    Nurbs(crate::geometry::nurbs::NurbsSurface),
}

impl HistoricalFaceSurface {
    /// The carrier of `geometry`, absent for a carrier that refers to other
    /// surfaces or holds no closed-form shape.
    #[must_use]
    pub fn from_solved(geometry: &crate::geometry::SolvedSurfaceGeometry) -> Option<Self> {
        use crate::geometry::SolvedSurfaceGeometry as Solved;
        match geometry {
            Solved::Plane(surface) => Some(Self::Plane(*surface)),
            Solved::Cylinder(surface) => Some(Self::Cylinder(*surface)),
            Solved::Cone(surface) => Some(Self::Cone(*surface)),
            Solved::Sphere(surface) => Some(Self::Sphere(*surface)),
            Solved::Torus(surface) => Some(Self::Torus(*surface)),
            Solved::Nurbs(surface) => Some(Self::Nurbs(surface.clone())),
            Solved::Polygonal(_) | Solved::Transformed(_) | Solved::Unknown { .. } => None,
        }
    }

    /// The carrier as solved surface geometry, for evaluation.
    #[must_use]
    pub fn to_solved(&self) -> crate::geometry::SolvedSurfaceGeometry {
        use crate::geometry::SolvedSurfaceGeometry as Solved;
        match self {
            Self::Plane(surface) => Solved::Plane(*surface),
            Self::Cylinder(surface) => Solved::Cylinder(*surface),
            Self::Cone(surface) => Solved::Cone(*surface),
            Self::Sphere(surface) => Solved::Sphere(*surface),
            Self::Torus(surface) => Solved::Torus(*surface),
            Self::Nurbs(surface) => Solved::Nurbs(surface.clone()),
        }
    }
}

/// One member of a feature result state: a body, face, edge, or vertex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SelectionMember {
    /// A feature-local body identity.
    Body {
        /// Feature-local identity.
        id: NonBlankString,
    },
    /// A feature-local face identity.
    Face {
        /// Feature-local identity.
        id: NonBlankString,
    },
    /// A feature-local edge identity.
    Edge {
        /// Feature-local identity.
        id: NonBlankString,
    },
    /// A feature-local vertex identity.
    Vertex {
        /// Feature-local identity.
        id: NonBlankString,
    },
}

/// The members of a feature result state, sorted into kinds.
///
/// The wire carries one non-empty member list, so "all four lists are empty"
/// has no spelling and no arm refuses it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    try_from = "NonEmptyMembers<SelectionMember>",
    into = "Vec<SelectionMember>"
)]
pub struct FeatureResultMembers {
    pub(super) bodies: Vec<NonBlankString>,
    pub(super) faces: Vec<NonBlankString>,
    pub(super) edges: Vec<NonBlankString>,
    pub(super) vertices: Vec<NonBlankString>,
}

/// Refusal for a feature result member list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeatureResultMemberError {
    /// The member list carries no member.
    #[error(transparent)]
    Members(#[from] BodySelectionError),
    /// A feature-local body identity occurs more than once.
    #[error("feature result bodies must be distinct")]
    RepeatedBody,
    /// A feature-local face identity occurs more than once.
    #[error("feature result faces must be distinct")]
    RepeatedFace,
    /// A feature-local edge identity occurs more than once.
    #[error("feature result edges must be distinct")]
    RepeatedEdge,
    /// A feature-local vertex identity occurs more than once.
    #[error("feature result vertices must be distinct")]
    RepeatedVertex,
}

impl TryFrom<NonEmptyMembers<SelectionMember>> for FeatureResultMembers {
    type Error = FeatureResultMemberError;

    fn try_from(members: NonEmptyMembers<SelectionMember>) -> Result<Self, Self::Error> {
        let mut sorted = Self {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        for member in members {
            let (target, id) = match member {
                SelectionMember::Body { id } => (&mut sorted.bodies, id),
                SelectionMember::Face { id } => (&mut sorted.faces, id),
                SelectionMember::Edge { id } => (&mut sorted.edges, id),
                SelectionMember::Vertex { id } => (&mut sorted.vertices, id),
            };
            target
                .try_reserve(1)
                .map_err(|_| BodySelectionError::Allocation)?;
            target.push(id);
        }
        sorted.validate()
    }
}

impl FeatureResultMembers {
    /// Admit distinct local members in each arena, retaining their source order.
    pub fn new(
        bodies: Vec<NonBlankString>,
        faces: Vec<NonBlankString>,
        edges: Vec<NonBlankString>,
        vertices: Vec<NonBlankString>,
    ) -> Result<Self, FeatureResultMemberError> {
        Self {
            bodies,
            faces,
            edges,
            vertices,
        }
        .validate()
    }

    fn validate(self) -> Result<Self, FeatureResultMemberError> {
        if self.bodies.is_empty()
            && self.faces.is_empty()
            && self.edges.is_empty()
            && self.vertices.is_empty()
        {
            return Err(BodySelectionError::Empty.into());
        }
        for (error, values) in [
            (FeatureResultMemberError::RepeatedBody, &self.bodies),
            (FeatureResultMemberError::RepeatedFace, &self.faces),
            (FeatureResultMemberError::RepeatedEdge, &self.edges),
            (FeatureResultMemberError::RepeatedVertex, &self.vertices),
        ] {
            if !membership::distinct(values, |_| true) {
                return Err(error);
            }
        }
        Ok(self)
    }
}

/// Persistent feature-local topology identities in one regenerated result.
///
/// These identities describe intermediate history results that need not be
/// members of the saved current model topology. Generated feature selections
/// address members through the producing feature and the corresponding local
/// identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeatureResultTopology {
    /// Globally unique result-state id.
    pub id: FeatureResultTopologyId,
    /// Feature that produces this state.
    pub output_of: FeatureId,
    /// Feature-local identities in stable source order, tagged by arena.
    #[cfg_attr(feature = "schema", schemars(with = "Vec<SelectionMember>"))]
    pub(super) members: FeatureResultMembers,
    /// Full-fidelity source state reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl FeatureResultTopology {
    /// Construct a result record from checked local members.
    pub fn new(
        id: FeatureResultTopologyId,
        output_of: FeatureId,
        members: FeatureResultMembers,
        native_ref: Option<String>,
    ) -> Self {
        Self {
            id,
            output_of,
            members,
            native_ref,
        }
    }

    /// Feature-local body identities.
    pub fn bodies(&self) -> &[NonBlankString] {
        &self.members.bodies
    }
    /// Feature-local face identities.
    pub fn faces(&self) -> &[NonBlankString] {
        &self.members.faces
    }
    /// Feature-local edge identities.
    pub fn edges(&self) -> &[NonBlankString] {
        &self.members.edges
    }
    /// Feature-local vertex identities.
    pub fn vertices(&self) -> &[NonBlankString] {
        &self.members.vertices
    }
}

impl HistoricalEdgeGeometry {
    /// Construct a sampled edge with its local identity and interior point.
    #[must_use]
    pub fn new(edge: HistoricalEdgeId, interior: FinitePoint3) -> Self {
        Self {
            edge,
            topology: None,
            carrier: None,
            interior,
            tangent: None,
            start: None,
            end: None,
            faces: DistinctMembers::default(),
            supports: Vec::new(),
        }
    }
}

/// Exact carrier path and adjacent-face correspondence in a consumed body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalEdgeTopologyRef {
    /// Body that holds the selected input state.
    #[cfg_attr(feature = "schema", schemars(extend("x-cadir-reference" = "model.history_bodies")))]
    pub history_body: HistoryBodyId,
    /// Exact edge or ordered outline.
    pub path: HistoricalEdgePath,
    /// Correspondence of state-local support faces to nest faces.
    pub faces: Vec<HistoricalFaceTopologyRef>,
}

/// Exact edge or ordered coedge ring inside one history-body nest.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoricalEdgePath {
    /// One edge and its admitted carrier interval.
    Edge {
        /// Edge in the selected nest.
        #[cfg_attr(feature = "schema", schemars(extend("x-cadir-reference" = "history_body.topology.edges")))]
        edge: EdgeId,
    },
    /// One closed ordered ring.
    Loop {
        /// Loop in the selected nest.
        #[cfg_attr(feature = "schema", schemars(extend("x-cadir-reference" = "history_body.topology.loops")))]
        loop_id: LoopId,
    },
}

/// Topology faces that represent one historical adjacent face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoricalFaceTopologyRef {
    /// Face in the owning input state.
    pub face: HistoricalFaceId,
    /// Faces in the selected history-body nest.
    #[cfg_attr(feature = "schema", schemars(extend("x-cadir-reference" = "history_body.topology.faces")))]
    pub topology_faces: NonEmptyMembers<FaceId>,
}

cadmpeg_core::named_optional_field!(
    deserialize_edge_geometry_topology,
    HistoricalEdgeTopologyRef,
    "topology"
);
