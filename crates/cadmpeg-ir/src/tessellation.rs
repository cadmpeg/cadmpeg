// SPDX-License-Identifier: Apache-2.0
//! Source tessellation retained alongside exact boundary representation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::math::{Point3, Vector3};
use crate::provenance::EntityMeta;

/// One indexed triangle mesh decoded from a source display or facet stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tessellation {
    /// Stable source-derived identifier.
    pub id: String,
    /// Vertex positions in document units.
    pub vertices: Vec<Point3>,
    /// Zero-based vertex indices, with source winding preserved.
    pub triangles: Vec<[u32; 3]>,
    #[serde(default)]
    pub strip_lengths: Vec<u32>,
    #[serde(default)]
    pub normals: Vec<Vector3>,
    #[serde(default)]
    pub channels: Vec<TessellationChannel>,
    /// Byte provenance.
    pub meta: EntityMeta,
}

/// One descriptor from the source tessellation table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TessellationChannel {
    pub item_size: u32,
    pub kind: u32,
    pub flags: u32,
    pub count: u32,
    pub data: Vec<u8>,
}
