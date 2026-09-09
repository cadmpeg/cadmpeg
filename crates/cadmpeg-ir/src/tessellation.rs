// SPDX-License-Identifier: Apache-2.0
//! Source tessellation retained alongside exact boundary representation.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::assets::AssetId;
use crate::ids::{BodyId, FaceId};
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;

crate::ids::id_type!(
    /// Stable tessellation identity.
    TessellationId
);

/// Admission error in a tessellation mesh or channel carrier.
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
    id: TessellationId,
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
    pub id: TessellationId,
    /// Body represented by this mesh, when known.
    pub body: Option<BodyId>,
    /// Faces represented by this mesh, empty when face-level ownership is unknown.
    pub faces: Vec<FaceId>,
    /// Source chordal deflection tolerance, when carried.
    chordal_deflection: Option<f64>,
    /// Native source-object identity and effective display metadata.
    pub source_object: Option<SourceObjectAssociation>,
    vertices: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    /// Undirected geometric feature edges.
    feature_edges: Vec<[u32; 2]>,
    topology: TessellationTopology,
    shading: TessellationNormals,
    /// Source face or region groups as an ordered partition of the triangle ordinals.
    triangle_groups: Vec<TessellationTriangleGroup>,
    /// Source texture resources assigned to disjoint sets of triangle ordinals.
    texture_assignments: Vec<TessellationTextureAssignment>,
    channels: Vec<TessellationChannel>,
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

fn require_finite_vertices(vertices: &[Point3]) -> Result<(), TessellationError> {
    if vertices
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return Err(tessellation_error(
            "vertices contain a non-finite coordinate",
        ));
    }
    Ok(())
}

fn require_finite_normals(normals: &[Vector3]) -> Result<(), TessellationError> {
    if normals
        .iter()
        .any(|normal| !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite())
    {
        return Err(tessellation_error(
            "normals contain a non-finite coordinate",
        ));
    }
    Ok(())
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

fn require_feature_edges(
    vertices: &[Point3],
    feature_edges: &[[u32; 2]],
) -> Result<(), TessellationError> {
    if feature_edges.iter().any(|edge| {
        edge[0] >= edge[1] || usize::try_from(edge[1]).map_or(true, |index| index >= vertices.len())
    }) || feature_edges.windows(2).any(|edges| edges[0] >= edges[1])
    {
        return Err(tessellation_error(
            "contains an invalid tessellation feature edge",
        ));
    }
    Ok(())
}

fn require_triangle_groups(
    triangles: &[[u32; 3]],
    triangle_groups: &[TessellationTriangleGroup],
) -> Result<(), TessellationError> {
    if triangle_groups.is_empty() {
        return Ok(());
    }
    let mut memberships = std::iter::repeat_n(false, triangles.len()).collect::<Vec<_>>();
    let mut source_ids = std::collections::BTreeSet::new();
    let valid = triangle_groups.iter().all(|group| {
        !group.triangles.is_empty()
            && group
                .triangles
                .windows(2)
                .all(|ordinals| ordinals[0] < ordinals[1])
            && group.triangles.iter().all(|ordinal| {
                usize::try_from(*ordinal)
                    .ok()
                    .and_then(|ordinal| memberships.get_mut(ordinal))
                    .is_some_and(|visited| {
                        if *visited {
                            return false;
                        }
                        *visited = true;
                        true
                    })
            })
            && group.source_id.as_ref().is_none_or(|source_id| {
                !source_id.is_empty() && source_ids.insert(source_id.as_str())
            })
    }) && memberships.iter().all(|visited| *visited);
    if !valid {
        return Err(tessellation_error(
            "contains an invalid tessellation triangle-group partition",
        ));
    }
    Ok(())
}

fn require_texture_assignments(
    triangles: &[[u32; 3]],
    texture_assignments: &[TessellationTextureAssignment],
) -> Result<(), TessellationError> {
    if texture_assignments.is_empty() {
        return Ok(());
    }
    let mut memberships = std::iter::repeat_n(false, triangles.len()).collect::<Vec<_>>();
    let mut source_ids = std::collections::BTreeSet::new();
    let mut anonymous_textures = std::collections::BTreeSet::new();
    let valid = texture_assignments.iter().all(|assignment| {
        !assignment.triangles.is_empty()
            && match assignment.source_id.as_deref() {
                Some(source_id) => !source_id.is_empty() && source_ids.insert(source_id),
                None => anonymous_textures.insert(&assignment.texture),
            }
            && assignment
                .triangles
                .windows(2)
                .all(|ordinals| ordinals[0] < ordinals[1])
            && assignment.triangles.iter().all(|ordinal| {
                usize::try_from(*ordinal)
                    .ok()
                    .and_then(|ordinal| memberships.get_mut(ordinal))
                    .is_some_and(|visited| {
                        if *visited {
                            return false;
                        }
                        *visited = true;
                        true
                    })
            })
    });
    if !valid {
        return Err(tessellation_error(
            "contains invalid tessellation texture assignments",
        ));
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
        require_finite_vertices(&vertices)?;
        require_triangle_indices(&vertices, &triangles)?;
        match &shading {
            TessellationNormals::None => {}
            TessellationNormals::PerVertex(normals) => {
                require_finite_normals(normals)?;
                if normals.len() != vertices.len() {
                    return Err(tessellation_error(
                        "tessellation normals do not match vertex count",
                    ));
                }
            }
            TessellationNormals::PerCorner(normals) => {
                require_finite_normals(normals)?;
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
            id: TessellationId::mint(id).map_err(|error| tessellation_error(error.to_string()))?,
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

    /// Atomically edit vertex positions while preserving finite coordinates.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnOnce(&mut [Point3]),
    ) -> Result<(), TessellationError> {
        let mut vertices = self.vertices.clone();
        edit(&mut vertices);
        require_finite_vertices(&vertices)?;
        self.vertices = vertices;
        Ok(())
    }

    /// Zero-based vertex indices, with source winding preserved.
    #[must_use]
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
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
    pub fn vertex_normals(&self) -> &[Vector3] {
        match &self.shading {
            TessellationNormals::PerVertex(normals) => normals,
            TessellationNormals::None | TessellationNormals::PerCorner(_) => &[],
        }
    }

    /// Atomically edit the stored shading normals while preserving finite coordinates.
    pub fn edit_normals(
        &mut self,
        edit: impl FnOnce(&mut [Vector3]),
    ) -> Result<(), TessellationError> {
        let mut shading = self.shading.clone();
        let normals = match &mut shading {
            TessellationNormals::PerVertex(normals) | TessellationNormals::PerCorner(normals) => {
                normals.as_mut_slice()
            }
            TessellationNormals::None => &mut [],
        };
        edit(normals);
        require_finite_normals(normals)?;
        self.shading = shading;
        Ok(())
    }

    /// Per-triangle-corner normals; empty when the source carried none or vertex normals.
    #[must_use]
    pub fn per_corner_normals(&self) -> &[Vector3] {
        match &self.shading {
            TessellationNormals::PerCorner(normals) => normals,
            TessellationNormals::None | TessellationNormals::PerVertex(_) => &[],
        }
    }

    /// Additional per-vertex or per-facet data channels.
    #[must_use]
    pub fn channels(&self) -> &[TessellationChannel] {
        &self.channels
    }

    /// Undirected geometric feature edges in source order.
    #[must_use]
    pub fn feature_edges(&self) -> &[[u32; 2]] {
        &self.feature_edges
    }

    /// Source face or region groups in triangle-order partition order.
    #[must_use]
    pub fn triangle_groups(&self) -> &[TessellationTriangleGroup] {
        &self.triangle_groups
    }

    /// Source texture resources assigned to triangle ordinals.
    #[must_use]
    pub fn texture_assignments(&self) -> &[TessellationTextureAssignment] {
        &self.texture_assignments
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

    /// Source chordal deflection tolerance.
    #[must_use]
    pub const fn chordal_deflection(&self) -> Option<f64> {
        self.chordal_deflection
    }

    /// Set a finite, non-negative source chordal deflection.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: Option<f64>,
    ) -> Result<(), TessellationError> {
        if chordal_deflection.is_some_and(|value| !value.is_finite() || value < 0.0) {
            return Err(tessellation_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        self.chordal_deflection = chordal_deflection;
        Ok(())
    }

    /// Set a finite, non-negative source chordal deflection.
    pub fn with_chordal_deflection(
        mut self,
        chordal_deflection: Option<f64>,
    ) -> Result<Self, TessellationError> {
        self.set_chordal_deflection(chordal_deflection)?;
        Ok(self)
    }

    /// Set the native source-object identity.
    #[must_use]
    pub fn with_source_object(mut self, source_object: Option<SourceObjectAssociation>) -> Self {
        self.source_object = source_object;
        self
    }

    /// Set the geometric feature edges.
    pub fn with_feature_edges(
        mut self,
        feature_edges: Vec<[u32; 2]>,
    ) -> Result<Self, TessellationError> {
        require_feature_edges(&self.vertices, &feature_edges)?;
        self.feature_edges = feature_edges;
        Ok(self)
    }

    /// Set the triangle-group partition.
    pub fn with_triangle_groups(
        mut self,
        triangle_groups: Vec<TessellationTriangleGroup>,
    ) -> Result<Self, TessellationError> {
        require_triangle_groups(&self.triangles, &triangle_groups)?;
        self.triangle_groups = triangle_groups;
        Ok(self)
    }

    /// Set the texture assignments.
    pub fn with_texture_assignments(
        mut self,
        texture_assignments: Vec<TessellationTextureAssignment>,
    ) -> Result<Self, TessellationError> {
        require_texture_assignments(&self.triangles, &texture_assignments)?;
        self.texture_assignments = texture_assignments;
        Ok(self)
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
        } else if !data.len().is_multiple_of(item_size_usize) {
            return Err(tessellation_error(
                "contains a malformed tessellation channel",
            ));
        }
        let count = u32::try_from(data.len().checked_div(item_size_usize).unwrap_or(0))
            .map_err(|_| tessellation_error("tessellation channel count overflows u32"))?;
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
        self.data.len().checked_div(item_size).unwrap_or(0) as u32
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
        let normals = mesh.vertex_normals().to_vec();
        let corner_normals = mesh.per_corner_normals().to_vec();
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
            wire.id.into_string(),
            wire.vertices,
            wire.triangles,
            topology,
            shading,
            wire.channels,
        )?;
        mesh.body = wire.body;
        mesh.faces = wire.faces;
        mesh.set_chordal_deflection(wire.chordal_deflection)?;
        mesh.source_object = wire.source_object;
        mesh = mesh.with_feature_edges(wire.feature_edges)?;
        mesh = mesh.with_triangle_groups(wire.triangle_groups)?;
        mesh.with_texture_assignments(wire.texture_assignments)
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

#[cfg(test)]
mod tests;
