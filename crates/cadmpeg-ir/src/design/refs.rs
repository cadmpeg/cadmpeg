// SPDX-License-Identifier: Apache-2.0
//! References to entities, records, positions, and states.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::ContainerId;
use crate::features::{ConfigurationId, FeatureId};
use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SubdId, SurfaceId, VertexId,
};

/// A nameable entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EntityRef {
    /// A body.
    Body(BodyId),
    /// A face.
    Face(FaceId),
    /// An edge.
    Edge(EdgeId),
    /// A vertex.
    Vertex(VertexId),
    /// A surface carrier.
    Surface(SurfaceId),
    /// A curve carrier.
    Curve(CurveId),
    /// A subdivision cage.
    Subd(SubdId),
}

impl EntityRef {
    /// The entity's record id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Body(id) => id.as_str(),
            Self::Face(id) => id.as_str(),
            Self::Edge(id) => id.as_str(),
            Self::Vertex(id) => id.as_str(),
            Self::Surface(id) => id.as_str(),
            Self::Curve(id) => id.as_str(),
            Self::Subd(id) => id.as_str(),
        }
    }
}

/// A record of any `Topology` arena, one variant per arena, for a delta's
/// `removed` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RecordRef {
    /// A record of the `bodies` arena.
    Body(BodyId),
    /// A record of the `regions` arena.
    Region(RegionId),
    /// A record of the `shells` arena.
    Shell(ShellId),
    /// A record of the `faces` arena.
    Face(FaceId),
    /// A record of the `loops` arena.
    Loop(LoopId),
    /// A record of the `coedges` arena.
    Coedge(CoedgeId),
    /// A record of the `edges` arena.
    Edge(EdgeId),
    /// A record of the `vertices` arena.
    Vertex(VertexId),
    /// A record of the `points` arena.
    Point(PointId),
    /// A record of the `surfaces` arena.
    Surface(SurfaceId),
    /// A record of the `curves` arena.
    Curve(CurveId),
    /// A record of the `subds` arena.
    Subd(SubdId),
    /// A record of the `pcurves` arena.
    Pcurve(PcurveId),
    /// A record of the `procedural_surfaces` arena.
    ProceduralSurface(ProceduralSurfaceId),
    /// A record of the `procedural_curves` arena.
    ProceduralCurve(ProceduralCurveId),
}

/// A configuration, of this document or by the source's name for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConfigurationRef {
    /// A configuration of this document.
    Local(ConfigurationId),
    /// A configuration by its name, which identifies it.
    Named(NonBlankString),
}

/// A member of a design container: a feature or a nested container.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Member {
    /// A feature.
    Feature(FeatureId),
    /// A nested container.
    Container(ContainerId),
}

/// A position in a design container.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Position {
    /// The start of a container, before its first member.
    Start(ContainerId),
    /// The position after a member.
    After(Member),
}

/// What an evaluation evaluates: the state at a position, or a tool body of
/// a feature.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Evaluated {
    /// The state at a position.
    State(Position),
    /// The tool body of a feature.
    Tool(FeatureId),
}

/// A state, by the evaluation that holds it, in a configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StateRef {
    /// The configuration of the state. `None` is the configuration in use.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationId>,
    /// What the state's evaluation evaluates.
    pub of: Evaluated,
}

cadmpeg_core::named_optional_field!(deserialize_configuration, ConfigurationId, "configuration");
