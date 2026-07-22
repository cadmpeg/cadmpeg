// SPDX-License-Identifier: Apache-2.0
//! Source tessellation retained alongside exact boundary representation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, FaceId};
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;

/// One indexed triangle mesh decoded from a source display or facet stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tessellation {
    /// Stable source-derived identifier.
    pub id: String,
    /// Body represented by this mesh, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyId>,
    /// Faces represented by this mesh, empty when face-level ownership is unknown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceId>,
    /// Source chordal deflection tolerance, when carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chordal_deflection: Option<f64>,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
    /// Vertex positions in document units.
    pub vertices: Vec<Point3>,
    /// Zero-based vertex indices, with source winding preserved.
    pub triangles: Vec<[u32; 3]>,
    /// Triangle-strip run lengths, when the source stored strips instead of an
    /// independent triangle list; empty when the mesh is a flat triangle list.
    #[serde(default)]
    pub strip_lengths: Vec<u32>,
    /// Per-vertex normals, parallel to `vertices`; empty when the source carried none.
    #[serde(default)]
    pub normals: Vec<Vector3>,
    /// Additional per-vertex or per-facet data channels from the source tessellation
    /// table (e.g. UVs, colors); empty when the source carried none.
    #[serde(default)]
    pub channels: Vec<TessellationChannel>,
}

/// One descriptor from the source tessellation table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TessellationChannel {
    /// Byte size of one element of `data`.
    pub item_size: u32,
    /// Source channel-kind tag (e.g. UV, color); interpretation is source-defined.
    pub kind: u32,
    /// Source per-channel flag word.
    pub flags: u32,
    /// Number of elements in `data`.
    pub count: u32,
    /// Raw channel payload, `count * item_size` bytes, undecoded.
    #[serde(with = "crate::bytes")]
    #[schemars(with = "String")]
    pub data: Vec<u8>,
}
