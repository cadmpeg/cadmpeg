// SPDX-License-Identifier: Apache-2.0
//! Feature evaluations and their input and result topology.

use super::geometry::FinitePoint3;
use super::membership;
use super::selections::{BodySelectionError, DistinctMembers, NonEmptyMembers};
use super::{
    deserialize_edge_geometry_end, deserialize_edge_geometry_start,
    deserialize_edge_geometry_tangent, deserialize_native_ref, FeatureDefinition, FeatureId,
};
use crate::ids::{
    BodyId, FeatureInputTopologyId, FeatureResultTopologyId, HistoricalBodyId, HistoricalEdgeId,
    HistoricalFaceId, HistoricalVertexId,
};
use crate::units::UnitVector3;
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// Full-fidelity source state reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
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
    /// Support faces of an unequal two-distance chamfer that selects this edge.
    /// Empty for every other edge.
    pub supports: Vec<EdgeSupport>,
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
