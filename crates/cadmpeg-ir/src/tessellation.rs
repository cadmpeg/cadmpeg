// SPDX-License-Identifier: Apache-2.0
//! Source tessellation retained alongside exact boundary representation.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::assets::AssetId;
use crate::ids::{BodyId, FaceId};
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;

/// Structural error in a tessellation mesh or channel carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TessellationError(String);

impl std::fmt::Display for TessellationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TessellationError {}

fn tessellation_error(message: impl Into<String>) -> TessellationError {
    TessellationError(message.into())
}

/// Shading samples stored with a tessellation mesh.
#[derive(Debug, Clone, PartialEq)]
pub enum TessellationNormals {
    /// The source carried no normals.
    None,
    /// One normal per vertex, parallel to [`Tessellation::vertices`].
    PerVertex(Vec<Vector3>),
    /// One normal per triangle corner, in flattened triangle order.
    PerCorner(Vec<Vector3>),
}

/// Triangle storage selected by the source mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TessellationTopology {
    /// Independent triangle list.
    List,
    /// Triangle-strip run lengths that expand to the stored triangles.
    Strips(Vec<u32>),
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

/// Index table that addresses a tessellation channel payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelAddressing {
    /// Vertex-order addressing with no explicit index table.
    Vertex,
    /// One selector per triangle corner.
    Corner(Vec<u32>),
    /// One selector per triangle.
    Triangle(Vec<u32>),
}

impl ChannelAddressing {
    /// Domain stored on the CADIR wire for this addressing.
    #[must_use]
    pub const fn domain(&self) -> TessellationChannelDomain {
        match self {
            Self::Vertex => TessellationChannelDomain::Vertex,
            Self::Corner(_) => TessellationChannelDomain::Corner,
            Self::Triangle(_) => TessellationChannelDomain::Triangle,
        }
    }

    /// Explicit selectors, empty for vertex-order addressing.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        match self {
            Self::Vertex => &[],
            Self::Corner(indices) | Self::Triangle(indices) => indices,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TessellationWire {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<BodyId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    faces: Vec<FaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chordal_deflection: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_object: Option<SourceObjectAssociation>,
    vertices: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    feature_edges: Vec<[u32; 2]>,
    #[serde(default)]
    strip_lengths: Vec<u32>,
    #[serde(default)]
    normals: Vec<Vector3>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    corner_normals: Vec<Vector3>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    triangle_groups: Vec<TessellationTriangleGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    texture_assignments: Vec<TessellationTextureAssignment>,
    #[serde(default)]
    channels: Vec<TessellationChannel>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TessellationChannelWire {
    #[serde(default, skip_serializing_if = "TessellationChannelDomain::is_vertex")]
    domain: TessellationChannelDomain,
    item_size: u32,
    kind: u32,
    flags: u32,
    count: u32,
    #[serde(with = "crate::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    data: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    indices: Vec<u32>,
}

/// One indexed triangle mesh decoded from a source display or facet stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TessellationWire", into = "TessellationWire")]
pub struct Tessellation {
    /// Stable source-derived identifier.
    pub id: String,
    /// Body represented by this mesh, when known.
    pub body: Option<BodyId>,
    /// Faces represented by this mesh, empty when face-level ownership is unknown.
    pub faces: Vec<FaceId>,
    /// Source chordal deflection tolerance, when carried.
    pub chordal_deflection: Option<f64>,
    /// Native source-object identity and effective display metadata.
    pub source_object: Option<SourceObjectAssociation>,
    pub(crate) vertices: Vec<Point3>,
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Undirected geometric feature edges.
    pub feature_edges: Vec<[u32; 2]>,
    pub(crate) topology: TessellationTopology,
    pub(crate) shading: TessellationNormals,
    /// Source face or region groups as an ordered partition of the triangle ordinals.
    pub triangle_groups: Vec<TessellationTriangleGroup>,
    /// Source texture resources assigned to disjoint sets of triangle ordinals.
    pub texture_assignments: Vec<TessellationTextureAssignment>,
    pub(crate) channels: Vec<TessellationChannel>,
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

/// One descriptor from the source tessellation table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TessellationChannelWire", into = "TessellationChannelWire")]
pub struct TessellationChannel {
    addressing: ChannelAddressing,
    item_size: u32,
    kind: u32,
    flags: u32,
    data: Vec<u8>,
}

fn triangles_from_strips(strips: &[u32]) -> Result<Vec<[u32; 3]>, TessellationError> {
    let mut triangles = Vec::new();
    let mut base = 0u32;
    for &length in strips {
        for index in 0..length.saturating_sub(2) {
            let Some(a) = base.checked_add(index) else {
                return Err(tessellation_error("tessellation strip index overflows u32"));
            };
            let Some(b) = a.checked_add(1) else {
                return Err(tessellation_error("tessellation strip index overflows u32"));
            };
            let Some(c) = a.checked_add(2) else {
                return Err(tessellation_error("tessellation strip index overflows u32"));
            };
            triangles.push(if index % 2 == 0 { [a, b, c] } else { [a, c, b] });
        }
        base = base
            .checked_add(length)
            .ok_or_else(|| tessellation_error("tessellation strip index overflows u32"))?;
    }
    Ok(triangles)
}

fn require_triangle_indices(
    vertices: &[Point3],
    triangles: &[[u32; 3]],
) -> Result<(), TessellationError> {
    if triangles
        .iter()
        .flatten()
        .any(|index| *index as usize >= vertices.len())
    {
        return Err(tessellation_error(
            "contains an out-of-range tessellation index",
        ));
    }
    Ok(())
}

fn shading_from_parts(
    vertices: &[Point3],
    triangles: &[[u32; 3]],
    normals: Vec<Vector3>,
    corner_normals: Vec<Vector3>,
) -> Result<TessellationNormals, TessellationError> {
    match (normals.is_empty(), corner_normals.is_empty()) {
        (true, true) => Ok(TessellationNormals::None),
        (false, true) => {
            if normals.len() != vertices.len() {
                return Err(tessellation_error(
                    "tessellation normals do not match vertex count",
                ));
            }
            Ok(TessellationNormals::PerVertex(normals))
        }
        (true, false) => {
            if triangles.len().checked_mul(3) != Some(corner_normals.len()) {
                return Err(tessellation_error(
                    "tessellation corner normals do not match triangle corners",
                ));
            }
            Ok(TessellationNormals::PerCorner(corner_normals))
        }
        (false, false) => Err(tessellation_error(
            "tessellation cannot store both vertex normals and corner normals",
        )),
    }
}

fn topology_from_parts(
    vertices: &[Point3],
    triangles: &[[u32; 3]],
    strip_lengths: Vec<u32>,
) -> Result<TessellationTopology, TessellationError> {
    if strip_lengths.is_empty() {
        return Ok(TessellationTopology::List);
    }
    let vertex_total = strip_lengths.iter().try_fold(0usize, |total, length| {
        usize::try_from(*length)
            .ok()
            .and_then(|length| total.checked_add(length))
    });
    if vertex_total != Some(vertices.len()) {
        return Err(tessellation_error(
            "tessellation strips do not match vertex count",
        ));
    }
    if triangles_from_strips(&strip_lengths)? != *triangles {
        return Err(tessellation_error(
            "tessellation triangles do not match strips",
        ));
    }
    Ok(TessellationTopology::Strips(strip_lengths))
}

fn require_channel_indices(
    triangles: &[[u32; 3]],
    channels: &[TessellationChannel],
) -> Result<(), TessellationError> {
    let corner_count = triangles
        .len()
        .checked_mul(3)
        .ok_or_else(|| tessellation_error("tessellation corner count overflows usize"))?;
    for channel in channels {
        let expected = match channel.addressing() {
            ChannelAddressing::Vertex => 0,
            ChannelAddressing::Corner(_) => corner_count,
            ChannelAddressing::Triangle(_) => triangles.len(),
        };
        if channel.indices().len() != expected
            || channel
                .indices()
                .iter()
                .any(|index| *index >= channel.count())
        {
            return Err(tessellation_error(
                "contains invalid tessellation channel indices",
            ));
        }
    }
    Ok(())
}

impl Tessellation {
    /// Build a tessellation whose shading, strip topology, and channels agree.
    pub fn new(
        id: impl Into<String>,
        vertices: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        topology: TessellationTopology,
        shading: TessellationNormals,
        channels: Vec<TessellationChannel>,
    ) -> Result<Self, TessellationError> {
        require_triangle_indices(&vertices, &triangles)?;
        match &shading {
            TessellationNormals::None => {}
            TessellationNormals::PerVertex(normals) => {
                if normals.len() != vertices.len() {
                    return Err(tessellation_error(
                        "tessellation normals do not match vertex count",
                    ));
                }
            }
            TessellationNormals::PerCorner(normals) => {
                if triangles.len().checked_mul(3) != Some(normals.len()) {
                    return Err(tessellation_error(
                        "tessellation corner normals do not match triangle corners",
                    ));
                }
            }
        }
        match &topology {
            TessellationTopology::List => {}
            TessellationTopology::Strips(strip_lengths) => {
                if strip_lengths.is_empty() {
                    return Err(tessellation_error(
                        "tessellation strip topology cannot be empty",
                    ));
                }
                let vertex_total = strip_lengths.iter().try_fold(0usize, |total, length| {
                    usize::try_from(*length)
                        .ok()
                        .and_then(|length| total.checked_add(length))
                });
                if vertex_total != Some(vertices.len()) {
                    return Err(tessellation_error(
                        "tessellation strips do not match vertex count",
                    ));
                }
                if triangles_from_strips(strip_lengths)? != triangles {
                    return Err(tessellation_error(
                        "tessellation triangles do not match strips",
                    ));
                }
            }
        }
        require_channel_indices(&triangles, &channels)?;
        Ok(Self {
            id: id.into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices,
            triangles,
            feature_edges: Vec::new(),
            topology,
            shading,
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels,
        })
    }

    /// Build from the CADIR-parallel shading and strip arrays.
    pub fn from_decoded(
        id: impl Into<String>,
        vertices: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        strip_lengths: Vec<u32>,
        normals: Vec<Vector3>,
        corner_normals: Vec<Vector3>,
        channels: Vec<TessellationChannel>,
    ) -> Result<Self, TessellationError> {
        let shading = shading_from_parts(&vertices, &triangles, normals, corner_normals)?;
        let topology = topology_from_parts(&vertices, &triangles, strip_lengths)?;
        Self::new(id, vertices, triangles, topology, shading, channels)
    }

    /// Vertex positions in document units.
    #[must_use]
    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }

    /// Mutable vertex positions. The cardinality cannot change.
    pub fn vertices_mut(&mut self) -> &mut [Point3] {
        &mut self.vertices
    }

    /// Zero-based vertex indices, with source winding preserved.
    #[must_use]
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// Mutable triangle indices. The cardinality cannot change.
    pub fn triangles_mut(&mut self) -> &mut [[u32; 3]] {
        &mut self.triangles
    }

    /// Triangle storage selected by the source mesh.
    #[must_use]
    pub fn topology(&self) -> &TessellationTopology {
        &self.topology
    }

    /// Triangle-strip run lengths; empty when the mesh is a flat triangle list.
    #[must_use]
    pub fn strip_lengths(&self) -> &[u32] {
        match &self.topology {
            TessellationTopology::List => &[],
            TessellationTopology::Strips(lengths) => lengths,
        }
    }

    /// Shading samples stored with this mesh.
    #[must_use]
    pub fn shading(&self) -> &TessellationNormals {
        &self.shading
    }

    /// Per-vertex normals; empty when the source carried none or corner normals.
    #[must_use]
    pub fn normals(&self) -> &[Vector3] {
        match &self.shading {
            TessellationNormals::PerVertex(normals) => normals,
            TessellationNormals::None | TessellationNormals::PerCorner(_) => &[],
        }
    }

    /// Mutable per-vertex normals. The cardinality cannot change.
    pub fn normals_mut(&mut self) -> Option<&mut [Vector3]> {
        match &mut self.shading {
            TessellationNormals::PerVertex(normals) => Some(normals),
            TessellationNormals::None | TessellationNormals::PerCorner(_) => None,
        }
    }

    /// Per-triangle-corner normals; empty when the source carried none or vertex normals.
    #[must_use]
    pub fn corner_normals(&self) -> &[Vector3] {
        match &self.shading {
            TessellationNormals::PerCorner(normals) => normals,
            TessellationNormals::None | TessellationNormals::PerVertex(_) => &[],
        }
    }

    /// Mutable per-triangle-corner normals. The cardinality cannot change.
    pub fn corner_normals_mut(&mut self) -> Option<&mut [Vector3]> {
        match &mut self.shading {
            TessellationNormals::PerCorner(normals) => Some(normals),
            TessellationNormals::None | TessellationNormals::PerVertex(_) => None,
        }
    }

    /// Additional per-vertex or per-facet data channels.
    #[must_use]
    pub fn channels(&self) -> &[TessellationChannel] {
        &self.channels
    }

    /// Mutable channel table. The channel count cannot change.
    pub fn channels_mut(&mut self) -> &mut [TessellationChannel] {
        &mut self.channels
    }

    /// Set the owning body.
    #[must_use]
    pub fn with_body(mut self, body: Option<BodyId>) -> Self {
        self.body = body;
        self
    }

    /// Set the represented faces.
    #[must_use]
    pub fn with_faces(mut self, faces: Vec<FaceId>) -> Self {
        self.faces = faces;
        self
    }

    /// Set the source chordal deflection.
    #[must_use]
    pub fn with_chordal_deflection(mut self, chordal_deflection: Option<f64>) -> Self {
        self.chordal_deflection = chordal_deflection;
        self
    }

    /// Set the native source-object identity.
    #[must_use]
    pub fn with_source_object(mut self, source_object: Option<SourceObjectAssociation>) -> Self {
        self.source_object = source_object;
        self
    }

    /// Set the geometric feature edges.
    #[must_use]
    pub fn with_feature_edges(mut self, feature_edges: Vec<[u32; 2]>) -> Self {
        self.feature_edges = feature_edges;
        self
    }

    /// Set the triangle-group partition.
    #[must_use]
    pub fn with_triangle_groups(mut self, triangle_groups: Vec<TessellationTriangleGroup>) -> Self {
        self.triangle_groups = triangle_groups;
        self
    }

    /// Set the texture assignments.
    #[must_use]
    pub fn with_texture_assignments(
        mut self,
        texture_assignments: Vec<TessellationTextureAssignment>,
    ) -> Self {
        self.texture_assignments = texture_assignments;
        self
    }
}

impl TessellationChannel {
    /// Build a channel whose payload length is an exact multiple of `item_size`.
    pub fn new(
        addressing: ChannelAddressing,
        item_size: u32,
        kind: u32,
        flags: u32,
        data: Vec<u8>,
    ) -> Result<Self, TessellationError> {
        let item_size_usize = usize::try_from(item_size)
            .map_err(|_| tessellation_error("tessellation channel item size overflows usize"))?;
        if item_size_usize == 0 {
            if !data.is_empty() {
                return Err(tessellation_error(
                    "contains a malformed tessellation channel",
                ));
            }
        } else if data.len() % item_size_usize != 0 {
            return Err(tessellation_error(
                "contains a malformed tessellation channel",
            ));
        }
        let count = if item_size_usize == 0 {
            0
        } else {
            u32::try_from(data.len() / item_size_usize)
                .map_err(|_| tessellation_error("tessellation channel count overflows u32"))?
        };
        if addressing.indices().iter().any(|index| *index >= count) {
            return Err(tessellation_error(
                "contains invalid tessellation channel indices",
            ));
        }
        Ok(Self {
            addressing,
            item_size,
            kind,
            flags,
            data,
        })
    }

    /// Mesh element addressed by this channel.
    #[must_use]
    pub fn addressing(&self) -> &ChannelAddressing {
        &self.addressing
    }

    /// Domain stored on the CADIR wire for this channel.
    #[must_use]
    pub fn domain(&self) -> TessellationChannelDomain {
        self.addressing.domain()
    }

    /// Byte size of one element of [`Self::data`].
    #[must_use]
    pub const fn item_size(&self) -> u32 {
        self.item_size
    }

    /// Source channel-kind tag.
    #[must_use]
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// Source per-channel flag word.
    #[must_use]
    pub const fn flags(&self) -> u32 {
        self.flags
    }

    /// Number of elements in [`Self::data`].
    #[must_use]
    pub fn count(&self) -> u32 {
        let item_size = self.item_size as usize;
        if item_size == 0 {
            0
        } else {
            (self.data.len() / item_size) as u32
        }
    }

    /// Raw channel payload.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Explicit selectors, empty for vertex-order addressing.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        self.addressing.indices()
    }
}

impl From<Tessellation> for TessellationWire {
    fn from(mesh: Tessellation) -> Self {
        let strip_lengths = mesh.strip_lengths().to_vec();
        let normals = mesh.normals().to_vec();
        let corner_normals = mesh.corner_normals().to_vec();
        Self {
            id: mesh.id,
            body: mesh.body,
            faces: mesh.faces,
            chordal_deflection: mesh.chordal_deflection,
            source_object: mesh.source_object,
            vertices: mesh.vertices,
            triangles: mesh.triangles,
            feature_edges: mesh.feature_edges,
            strip_lengths,
            normals,
            corner_normals,
            triangle_groups: mesh.triangle_groups,
            texture_assignments: mesh.texture_assignments,
            channels: mesh.channels,
        }
    }
}

impl TryFrom<TessellationWire> for Tessellation {
    type Error = TessellationError;

    fn try_from(wire: TessellationWire) -> Result<Self, Self::Error> {
        let shading = shading_from_parts(
            &wire.vertices,
            &wire.triangles,
            wire.normals,
            wire.corner_normals,
        )?;
        let topology = topology_from_parts(&wire.vertices, &wire.triangles, wire.strip_lengths)?;
        let mut mesh = Self::new(
            wire.id,
            wire.vertices,
            wire.triangles,
            topology,
            shading,
            wire.channels,
        )?;
        mesh.body = wire.body;
        mesh.faces = wire.faces;
        mesh.chordal_deflection = wire.chordal_deflection;
        mesh.source_object = wire.source_object;
        mesh.feature_edges = wire.feature_edges;
        mesh.triangle_groups = wire.triangle_groups;
        mesh.texture_assignments = wire.texture_assignments;
        Ok(mesh)
    }
}

impl From<TessellationChannel> for TessellationChannelWire {
    fn from(channel: TessellationChannel) -> Self {
        let domain = channel.domain();
        let count = channel.count();
        let indices = channel.indices().to_vec();
        Self {
            domain,
            item_size: channel.item_size,
            kind: channel.kind,
            flags: channel.flags,
            count,
            data: channel.data,
            indices,
        }
    }
}

impl TryFrom<TessellationChannelWire> for TessellationChannel {
    type Error = TessellationError;

    fn try_from(wire: TessellationChannelWire) -> Result<Self, Self::Error> {
        let item_size = usize::try_from(wire.item_size)
            .map_err(|_| tessellation_error("tessellation channel item size overflows usize"))?;
        let count = usize::try_from(wire.count)
            .map_err(|_| tessellation_error("tessellation channel count overflows usize"))?;
        let expected_len = item_size
            .checked_mul(count)
            .ok_or_else(|| tessellation_error("tessellation channel size overflow"))?;
        if wire.data.len() != expected_len {
            return Err(tessellation_error(
                "contains a malformed tessellation channel",
            ));
        }
        let addressing = match wire.domain {
            TessellationChannelDomain::Vertex => {
                if !wire.indices.is_empty() {
                    return Err(tessellation_error(
                        "contains invalid tessellation channel indices",
                    ));
                }
                ChannelAddressing::Vertex
            }
            TessellationChannelDomain::Corner => ChannelAddressing::Corner(wire.indices),
            TessellationChannelDomain::Triangle => ChannelAddressing::Triangle(wire.indices),
        };
        Self::new(addressing, wire.item_size, wire.kind, wire.flags, wire.data)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Tessellation {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Tessellation".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::Tessellation").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        TessellationWire::json_schema(generator)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for TessellationChannel {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "TessellationChannel".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::TessellationChannel").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        TessellationChannelWire::json_schema(generator)
    }
}
