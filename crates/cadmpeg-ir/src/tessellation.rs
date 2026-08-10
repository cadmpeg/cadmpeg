// SPDX-License-Identifier: Apache-2.0
//! Source tessellation retained alongside exact boundary representation.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::assets::AssetId;
use crate::ids::{BodyId, FaceId};
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;

/// One indexed triangle mesh decoded from a source display or facet stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    /// Undirected geometric feature edges, as a lexicographically sorted list
    /// of unique ascending pairs of zero-based vertex indices. The list
    /// excludes ordinary triangulation edges unless the source classifies them
    /// as features.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feature_edges: Vec<[u32; 2]>,
    /// Triangle-strip run lengths, when the source stored strips instead of an
    /// independent triangle list; empty when the mesh is a flat triangle list.
    #[serde(default)]
    pub strip_lengths: Vec<u32>,
    /// Per-vertex normals, parallel to `vertices`; empty when the source carried none.
    #[serde(default)]
    pub normals: Vec<Vector3>,
    /// Per-triangle-corner normals in flattened `triangles` order. These values
    /// preserve normal seams that cannot be represented by `normals` without
    /// duplicating vertices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corner_normals: Vec<Vector3>,
    /// Source face or region groups as an ordered partition of the triangle
    /// ordinals. Empty when the source carries no group partition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triangle_groups: Vec<TessellationTriangleGroup>,
    /// Source texture resources and assets assigned to disjoint sets of
    /// triangle ordinals. Omitted triangles have no direct texture assignment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texture_assignments: Vec<TessellationTextureAssignment>,
    /// Additional per-vertex or per-facet data channels from the source tessellation
    /// table (e.g. UVs, colors); empty when the source carried none.
    #[serde(default)]
    pub channels: Vec<TessellationChannel>,
}

/// One source-defined group in a tessellation triangle partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TessellationTriangleGroup {
    /// Source group identity, when the source stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Strictly increasing triangle ordinals belonging to this group.
    pub triangles: Vec<u32>,
}

/// One source texture resource assigned directly to tessellation triangles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TessellationTextureAssignment {
    /// Source texture-resource identity, when the source stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Assigned texture asset.
    pub texture: AssetId,
    /// Strictly increasing triangle ordinals receiving the texture.
    pub triangles: Vec<u32>,
}

/// The mesh element addressed by one tessellation channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TessellationChannelDomain {
    /// One channel value is associated with each tessellation vertex.
    #[default]
    Vertex,
    /// Each triangle corner selects one value from the channel table.
    Corner,
    /// Each triangle selects one value from the channel table.
    Triangle,
}

impl TessellationChannelDomain {
    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "Serde skip_serializing_if requires a reference predicate."
    )]
    fn is_vertex(&self) -> bool {
        matches!(self, Self::Vertex)
    }
}

/// One descriptor from the source tessellation table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TessellationChannel {
    /// The mesh element addressed by this channel. Omitted JSON fields use
    /// [`TessellationChannelDomain::Vertex`] for compatibility with IR v5.
    #[serde(default, skip_serializing_if = "TessellationChannelDomain::is_vertex")]
    pub domain: TessellationChannelDomain,
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
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub data: Vec<u8>,
    /// For corner and triangle channels, one selector per addressed mesh
    /// element. An empty vector means that a vertex channel uses its implicit
    /// vertex-order addressing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indices: Vec<u32>,
}
