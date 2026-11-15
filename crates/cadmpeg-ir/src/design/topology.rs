// SPDX-License-Identifier: Apache-2.0
//! A body's topology and geometry, as a pick's witness holds it.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geometry::pcurve::Pcurve;
use crate::geometry::{Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::subd::SubdSurface;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};

/// The records of one state's topology and geometry, one arena per record
/// kind.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Topology {
    /// The bodies.
    pub bodies: Vec<Body>,
    /// The regions.
    pub regions: Vec<Region>,
    /// The shells.
    pub shells: Vec<Shell>,
    /// The faces.
    pub faces: Vec<Face>,
    /// The loops.
    pub loops: Vec<Loop>,
    /// The coedges.
    pub coedges: Vec<Coedge>,
    /// The edges.
    pub edges: Vec<Edge>,
    /// The vertices.
    pub vertices: Vec<Vertex>,
    /// The points.
    pub points: Vec<Point>,
    /// The surface carriers.
    pub surfaces: Vec<Surface>,
    /// The curve carriers.
    pub curves: Vec<Curve>,
    /// The subdivision cages.
    pub subds: Vec<SubdSurface>,
    /// The pcurves.
    pub pcurves: Vec<Pcurve>,
    /// The procedural surface carriers.
    pub procedural_surfaces: Vec<ProceduralSurface>,
    /// The procedural curve carriers.
    pub procedural_curves: Vec<ProceduralCurve>,
}
