// SPDX-License-Identifier: Apache-2.0
//! History bodies: body states that only an earlier history state holds.
//!
//! A history body carries its whole topology and geometry in a nest of its
//! own. Every reference inside the nest resolves inside the nest, so the
//! shared topology arenas of the model never hold a record of a history body.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::FeatureId;
use crate::geometry::{pcurve::Pcurve, Curve, Surface};
use crate::ids::HistoryBodyId;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};

/// A state of a source history chain, by position and source identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoryStateRef {
    /// Position in the chain, oldest state first.
    pub ordinal: u32,
    /// Identifier that the source assigns to the state.
    pub identifier: i64,
}

/// The topology and geometry of one body. References resolve only among
/// these records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BodyTopology {
    /// The body.
    pub body: Body,
    /// Regions of the body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<Region>,
    /// Shells of the regions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shells: Vec<Shell>,
    /// Faces of the shells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<Face>,
    /// Loops of the faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<Loop>,
    /// Coedges of the loops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coedges: Vec<Coedge>,
    /// Edges of the coedges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<Edge>,
    /// Vertices of the edges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertices: Vec<Vertex>,
    /// Surface carriers of the faces. A carrier states solved geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<Surface>,
    /// Curve carriers of the edges and coedges. A carrier states solved geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<Curve>,
    /// Parameter-space curves of the coedges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<Pcurve>,
    /// Points of the vertices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
}

impl BodyTopology {
    /// A nest that holds `body` and no other record.
    #[must_use]
    pub fn new(body: Body) -> Self {
        Self {
            body,
            regions: Vec::new(),
            shells: Vec::new(),
            faces: Vec::new(),
            loops: Vec::new(),
            coedges: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
            surfaces: Vec::new(),
            curves: Vec::new(),
            pcurves: Vec::new(),
            points: Vec::new(),
        }
    }

    /// Total number of records in the nest.
    #[must_use]
    pub fn record_count(&self) -> usize {
        1 + self.regions.len()
            + self.shells.len()
            + self.faces.len()
            + self.loops.len()
            + self.coedges.len()
            + self.edges.len()
            + self.vertices.len()
            + self.surfaces.len()
            + self.curves.len()
            + self.pcurves.len()
            + self.points.len()
    }

    /// The nest as a model that holds only its records, for the topology checks.
    #[must_use]
    pub fn to_model(&self) -> crate::document::Model {
        let mut model = crate::document::Model::default();
        model.bodies.push(self.body.clone());
        model.regions.clone_from(&self.regions);
        model.shells.clone_from(&self.shells);
        model.faces.clone_from(&self.faces);
        model.loops.clone_from(&self.loops);
        model.coedges.clone_from(&self.coedges);
        model.edges.clone_from(&self.edges);
        model.vertices.clone_from(&self.vertices);
        model.surfaces.clone_from(&self.surfaces);
        model.curves.clone_from(&self.curves);
        model.pcurves.clone_from(&self.pcurves);
        model.points.clone_from(&self.points);
        model
    }
}

/// A body that only an earlier history state holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HistoryBody {
    /// Identity in the history-body namespace, which no top-level record shares.
    pub id: HistoryBodyId,
    /// Feature whose result is this body.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_produced_by"
    )]
    pub produced_by: Option<FeatureId>,
    /// Feature that consumes this body.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_consumed_by"
    )]
    pub consumed_by: Option<FeatureId>,
    /// History state that holds the body.
    pub state: HistoryStateRef,
    /// Records of the body.
    pub topology: Box<BodyTopology>,
}

cadmpeg_core::named_optional_field!(deserialize_produced_by, FeatureId, "produced_by");
cadmpeg_core::named_optional_field!(deserialize_consumed_by, FeatureId, "consumed_by");

mod identity_rewrite;
