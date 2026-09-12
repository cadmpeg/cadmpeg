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

/// One mesh vertex carrying its shading normal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShadedVertex {
    /// Vertex position in document units.
    pub position: Point3,
    /// Shading normal at this vertex.
    pub normal: Vector3,
}

/// One triangle carrying a shading normal at each of its corners.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShadedTriangle {
    /// Zero-based vertex indices, with source winding preserved.
    pub corners: [u32; 3],
    /// Shading normal at each corner, in corner order.
    pub normals: [Vector3; 3],
}

/// The position every mesh vertex carries, whatever else it carries.
pub trait MeshVertex {
    /// Position of this vertex in document units.
    fn position(&self) -> Point3;

    /// Position of this vertex, for editing in place.
    fn position_mut(&mut self) -> &mut Point3;
}

impl MeshVertex for Point3 {
    fn position(&self) -> Point3 {
        *self
    }

    fn position_mut(&mut self) -> &mut Point3 {
        self
    }
}

impl MeshVertex for ShadedVertex {
    fn position(&self) -> Point3 {
        self.position
    }

    fn position_mut(&mut self) -> &mut Point3 {
        &mut self.position
    }
}

/// One triangle strip: the vertices it spans, at least three of them.
///
/// A strip of one or two vertices spans no triangle, so it is not a strip at
/// all; the mint refuses it and [`Self::triangle_count`] subtracts without a
/// clamp on a length the type proved is at least three.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Strip<V>(Vec<V>);

impl<V> Strip<V> {
    /// A strip over `vertices`, absent below three.
    #[must_use]
    pub fn new(vertices: Vec<V>) -> Option<Self> {
        (vertices.len() >= 3).then_some(Self(vertices))
    }

    /// The strip's vertices in strip order.
    #[must_use]
    pub fn vertices(&self) -> &[V] {
        &self.0
    }

    /// The strip's vertices in strip order, for editing in place.
    pub fn vertices_mut(&mut self) -> &mut [V] {
        &mut self.0
    }

    /// The triangles this strip expands to: two fewer than its vertices.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.0.len() - 2
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Strip<V> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(Vec::<V>::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("tessellation strip spans no triangle"))
    }
}

/// One or more triangle strips.
///
/// The vector is private and never empty, so "the mesh is a flat triangle
/// list" has no second spelling inside a strip mesh. The mint also proves the
/// strips span no more vertices than a `u32` index can name, so expanding them
/// into triangles is total.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Strips<V>(Vec<Strip<V>>);

impl<V> Strips<V> {
    /// The strips in mesh order, absent when there are none and absent when
    /// they span more vertices than a `u32` index can name.
    #[must_use]
    pub fn new(strips: Vec<Strip<V>>) -> Option<Self> {
        if strips.is_empty() {
            return None;
        }
        let span = strips.iter().try_fold(0u64, |total, strip| {
            total.checked_add(strip.vertices().len() as u64)
        })?;
        (span <= u64::from(u32::MAX)).then_some(Self(strips))
    }

    /// The strips in mesh order.
    #[must_use]
    pub fn as_slice(&self) -> &[Strip<V>] {
        &self.0
    }

    /// The strips in mesh order, for editing in place.
    pub fn as_mut_slice(&mut self) -> &mut [Strip<V>] {
        &mut self.0
    }
}

impl<V> Strips<V> {
    /// Cut a flat vertex lane into the strips `spans` names.
    ///
    /// A source that states its strip spans beside one vertex array pairs the
    /// two here, once, at the decode boundary: the result is absent when a
    /// span the lane cannot fill, a span below three, or a vertex the spans do
    /// not consume.
    #[must_use]
    pub fn from_spans(vertices: Vec<V>, spans: &[u32]) -> Option<Self> {
        let mut remaining = vertices.into_iter();
        let mut strips = Vec::with_capacity(spans.len());
        for span in spans {
            let span = usize::try_from(*span).ok()?;
            let run: Vec<V> = remaining.by_ref().take(span).collect();
            if run.len() != span {
                return None;
            }
            strips.push(Strip::new(run)?);
        }
        if remaining.next().is_some() {
            return None;
        }
        Self::new(strips)
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Strips<V> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(Vec::<Strip<V>>::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("tessellation strip set is empty"))
    }
}

/// The mesh's vertices, triangles and shading normals.
///
/// The shading form is the wire's tag, so a mesh states per-vertex or
/// per-corner normals and never both, and never a normal run that does not
/// cover the mesh. A strip owns the vertices it spans, so a strip mesh states
/// neither a triangle list nor a vertex total: both are derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum TessellationMesh {
    /// Independent triangle list carrying no shading normals.
    List {
        /// Vertex positions in document units.
        vertices: Vec<Point3>,
        /// Zero-based vertex indices, with source winding preserved.
        triangles: Vec<[u32; 3]>,
    },
    /// Independent triangle list with one shading normal per vertex.
    ShadedList {
        /// Vertex rows, each carrying its shading normal.
        vertices: Vec<ShadedVertex>,
        /// Zero-based vertex indices, with source winding preserved.
        triangles: Vec<[u32; 3]>,
    },
    /// Independent triangle list with one shading normal per triangle corner.
    CornerShadedList {
        /// Vertex positions in document units.
        vertices: Vec<Point3>,
        /// Triangle rows, each carrying its three corner normals.
        triangles: Vec<ShadedTriangle>,
    },
    /// Triangle strips carrying no shading normals.
    Strips {
        /// Strips in mesh order; each spans the vertices it owns.
        strips: Strips<Point3>,
    },
    /// Triangle strips with one shading normal per vertex.
    ShadedStrips {
        /// Strips in mesh order; each spans the vertex rows it owns.
        strips: Strips<ShadedVertex>,
    },
}

/// Expand strip runs into triangles in strip order.
///
/// [`Strips::new`] proves the strips span no more vertices than a `u32` index
/// can name, so every index below is in range and the expansion is total.
fn strip_triangles<V>(strips: &Strips<V>) -> Vec<[u32; 3]> {
    let mut triangles = Vec::new();
    let mut base: u32 = 0;
    for strip in strips.as_slice() {
        for index in 0..strip.triangle_count() as u32 {
            let a = base + index;
            triangles.push(if index % 2 == 0 {
                [a, a + 1, a + 2]
            } else {
                [a, a + 2, a + 1]
            });
        }
        base += strip.vertices().len() as u32;
    }
    triangles
}

impl TessellationMesh {
    /// Pair a source's flat strip lanes into mesh rows.
    ///
    /// A display stream that states strip spans, vertex positions and vertex
    /// normals as separate lanes pairs them here. An empty normal lane states
    /// an unshaded mesh; any other normal lane covers the vertices exactly.
    #[must_use]
    pub fn from_strip_lanes(
        positions: Vec<Point3>,
        normals: Vec<Vector3>,
        spans: &[u32],
    ) -> Option<Self> {
        if normals.is_empty() {
            return Strips::from_spans(positions, spans).map(|strips| Self::Strips { strips });
        }
        if normals.len() != positions.len() {
            return None;
        }
        let rows = positions
            .into_iter()
            .zip(normals)
            .map(|(position, normal)| ShadedVertex { position, normal })
            .collect();
        Strips::from_spans(rows, spans).map(|strips| Self::ShadedStrips { strips })
    }

    /// Pair a source's flat triangle-list lanes into mesh rows.
    ///
    /// An empty normal lane states an unshaded mesh; any other normal lane
    /// covers the vertices exactly.
    #[must_use]
    pub fn from_list_lanes(
        positions: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        normals: Vec<Vector3>,
    ) -> Option<Self> {
        if normals.is_empty() {
            return Some(Self::List {
                vertices: positions,
                triangles,
            });
        }
        if normals.len() != positions.len() {
            return None;
        }
        Some(Self::ShadedList {
            vertices: positions
                .into_iter()
                .zip(normals)
                .map(|(position, normal)| ShadedVertex { position, normal })
                .collect(),
            triangles,
        })
    }

    /// Pair a source's flat triangle-list lanes and corner normals into rows.
    ///
    /// An empty normal lane states an unshaded mesh; any other normal lane
    /// covers the triangle corners exactly.
    #[must_use]
    pub fn from_corner_lanes(
        positions: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        corner_normals: Vec<Vector3>,
    ) -> Option<Self> {
        if corner_normals.is_empty() {
            return Some(Self::List {
                vertices: positions,
                triangles,
            });
        }
        if corner_normals.len() != triangles.len().checked_mul(3)? {
            return None;
        }
        let mut normals = corner_normals.into_iter();
        let rows = triangles
            .into_iter()
            .map(|corners| {
                Some(ShadedTriangle {
                    corners,
                    normals: [normals.next()?, normals.next()?, normals.next()?],
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self::CornerShadedList {
            vertices: positions,
            triangles: rows,
        })
    }

    /// Vertex positions in mesh order.
    #[must_use]
    pub fn vertices(&self) -> Vec<Point3> {
        match self {
            Self::List { vertices, .. } | Self::CornerShadedList { vertices, .. } => {
                vertices.clone()
            }
            Self::ShadedList { vertices, .. } => {
                vertices.iter().map(|vertex| vertex.position).collect()
            }
            Self::Strips { strips } => strips
                .as_slice()
                .iter()
                .flat_map(|strip| strip.vertices().iter().copied())
                .collect(),
            Self::ShadedStrips { strips } => strips
                .as_slice()
                .iter()
                .flat_map(|strip| strip.vertices().iter().map(|vertex| vertex.position))
                .collect(),
        }
    }

    /// Number of vertices in the mesh.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        match self {
            Self::List { vertices, .. } | Self::CornerShadedList { vertices, .. } => vertices.len(),
            Self::ShadedList { vertices, .. } => vertices.len(),
            Self::Strips { strips } => strips
                .as_slice()
                .iter()
                .map(|strip| strip.vertices().len())
                .sum(),
            Self::ShadedStrips { strips } => strips
                .as_slice()
                .iter()
                .map(|strip| strip.vertices().len())
                .sum(),
        }
    }

    /// Zero-based triangle corner indices, with source winding preserved.
    ///
    /// A strip mesh derives its triangles from the strips it owns.
    #[must_use]
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        match self {
            Self::List { triangles, .. } | Self::ShadedList { triangles, .. } => triangles.clone(),
            Self::CornerShadedList { triangles, .. } => {
                triangles.iter().map(|triangle| triangle.corners).collect()
            }
            Self::Strips { strips } => strip_triangles(strips),
            Self::ShadedStrips { strips } => strip_triangles(strips),
        }
    }

    /// Number of triangles in the mesh.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        match self {
            Self::List { triangles, .. } | Self::ShadedList { triangles, .. } => triangles.len(),
            Self::CornerShadedList { triangles, .. } => triangles.len(),
            Self::Strips { strips } => strips
                .as_slice()
                .iter()
                .map(Strip::triangle_count)
                .sum(),
            Self::ShadedStrips { strips } => strips
                .as_slice()
                .iter()
                .map(Strip::triangle_count)
                .sum(),
        }
    }

    /// Per-vertex shading normals; empty when the mesh carries none.
    #[must_use]
    pub fn vertex_normals(&self) -> Vec<Vector3> {
        match self {
            Self::List { .. } | Self::CornerShadedList { .. } | Self::Strips { .. } => Vec::new(),
            Self::ShadedList { vertices, .. } => {
                vertices.iter().map(|vertex| vertex.normal).collect()
            }
            Self::ShadedStrips { strips } => strips
                .as_slice()
                .iter()
                .flat_map(|strip| strip.vertices().iter().map(|vertex| vertex.normal))
                .collect(),
        }
    }

    /// Per-corner shading normals in flattened triangle order; empty when the
    /// mesh carries none.
    #[must_use]
    pub fn corner_normals(&self) -> Vec<Vector3> {
        match self {
            Self::List { .. }
            | Self::ShadedList { .. }
            | Self::Strips { .. }
            | Self::ShadedStrips { .. } => Vec::new(),
            Self::CornerShadedList { triangles, .. } => {
                triangles.iter().flat_map(|triangle| triangle.normals).collect()
            }
        }
    }

    /// Strip spans in mesh order; empty when the mesh is a triangle list.
    #[must_use]
    pub fn strip_lengths(&self) -> Vec<u32> {
        let spans = |lengths: Vec<usize>| {
            lengths
                .into_iter()
                .filter_map(|length| u32::try_from(length).ok())
                .collect()
        };
        match self {
            Self::List { .. } | Self::ShadedList { .. } | Self::CornerShadedList { .. } => {
                Vec::new()
            }
            Self::Strips { strips } => spans(
                strips
                    .as_slice()
                    .iter()
                    .map(|strip| strip.vertices().len())
                    .collect(),
            ),
            Self::ShadedStrips { strips } => spans(
                strips
                    .as_slice()
                    .iter()
                    .map(|strip| strip.vertices().len())
                    .collect(),
            ),
        }
    }

    /// Edit every vertex position in place.
    pub fn edit_positions(&mut self, mut edit: impl FnMut(&mut Point3)) {
        match self {
            Self::List { vertices, .. } | Self::CornerShadedList { vertices, .. } => {
                vertices.iter_mut().for_each(&mut edit);
            }
            Self::ShadedList { vertices, .. } => vertices
                .iter_mut()
                .for_each(|vertex| edit(&mut vertex.position)),
            Self::Strips { strips } => strips
                .as_mut_slice()
                .iter_mut()
                .for_each(|strip| strip.vertices_mut().iter_mut().for_each(&mut edit)),
            Self::ShadedStrips { strips } => {
                strips.as_mut_slice().iter_mut().for_each(|strip| {
                    strip
                        .vertices_mut()
                        .iter_mut()
                        .for_each(|vertex| edit(&mut vertex.position));
                });
            }
        }
    }

    /// Edit every shading normal in place; absent when the mesh carries none.
    pub fn edit_normals(&mut self, mut edit: impl FnMut(&mut Vector3)) -> bool {
        match self {
            Self::List { .. } | Self::Strips { .. } => false,
            Self::ShadedList { vertices, .. } => {
                vertices
                    .iter_mut()
                    .for_each(|vertex| edit(&mut vertex.normal));
                true
            }
            Self::CornerShadedList { triangles, .. } => {
                triangles
                    .iter_mut()
                    .for_each(|triangle| triangle.normals.iter_mut().for_each(&mut edit));
                true
            }
            Self::ShadedStrips { strips } => {
                strips.as_mut_slice().iter_mut().for_each(|strip| {
                    strip
                        .vertices_mut()
                        .iter_mut()
                        .for_each(|vertex| edit(&mut vertex.normal));
                });
                true
            }
        }
    }
}

/// The mesh element addressed by one tessellation channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub enum TessellationChannelDomain {
    /// One channel value is associated with each tessellation vertex.
    #[default]
    Vertex,
    /// Each triangle corner selects one value from the channel table.
    Corner,
    /// Each triangle selects one value from the channel table.
    Triangle,
}

/// Index table that addresses a tessellation channel payload.
///
/// The domain is the wire's tag, so the selector table exists only on the two
/// domains that address one: vertex-order addressing has no `indices` key to
/// leave empty and none to refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "domain", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ChannelAddressing {
    /// Vertex-order addressing with no explicit index table.
    Vertex {},
    /// One selector per triangle corner.
    Corner {
        /// One selector per triangle corner.
        indices: Vec<u32>,
    },
    /// One selector per triangle.
    Triangle {
        /// One selector per triangle.
        indices: Vec<u32>,
    },
}

impl ChannelAddressing {
    /// Domain stored on the CADIR wire for this addressing.
    #[must_use]
    pub const fn domain(&self) -> TessellationChannelDomain {
        match self {
            Self::Vertex {} => TessellationChannelDomain::Vertex,
            Self::Corner { .. } => TessellationChannelDomain::Corner,
            Self::Triangle { .. } => TessellationChannelDomain::Triangle,
        }
    }

    /// Explicit selectors, empty for vertex-order addressing.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        match self {
            Self::Vertex {} => &[],
            Self::Corner { indices } | Self::Triangle { indices } => indices,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
    mesh: TessellationMesh,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    feature_edges: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    triangle_groups: Vec<TessellationTriangleGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    texture_assignments: Vec<TessellationTextureAssignment>,
    #[serde(default)]
    channels: Vec<TessellationChannel>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TessellationChannelWire {
    addressing: ChannelAddressing,
    item_size: u32,
    kind: u32,
    flags: u32,
    #[serde(with = "crate::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    data: Vec<u8>,
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
    mesh: TessellationMesh,
    /// Undirected geometric feature edges.
    feature_edges: Vec<[u32; 2]>,
    /// Source face or region groups as an ordered partition of the triangle ordinals.
    triangle_groups: Vec<TessellationTriangleGroup>,
    /// Source texture resources assigned to disjoint sets of triangle ordinals.
    texture_assignments: Vec<TessellationTextureAssignment>,
    channels: Vec<TessellationChannel>,
}

/// One source-defined group in a tessellation triangle partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    vertex_count: usize,
    triangles: &[[u32; 3]],
) -> Result<(), TessellationError> {
    if triangles
        .iter()
        .flatten()
        .any(|index| *index as usize >= vertex_count)
    {
        return Err(tessellation_error(
            "contains an out-of-range tessellation index",
        ));
    }
    Ok(())
}

fn require_channel_indices(
    triangle_count: usize,
    channels: &[TessellationChannel],
) -> Result<(), TessellationError> {
    let corner_count = triangle_count
        .checked_mul(3)
        .ok_or_else(|| tessellation_error("tessellation corner count overflows usize"))?;
    for channel in channels {
        let expected = match channel.addressing() {
            ChannelAddressing::Vertex {} => 0,
            ChannelAddressing::Corner { .. } => corner_count,
            ChannelAddressing::Triangle { .. } => triangle_count,
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
    vertex_count: usize,
    feature_edges: &[[u32; 2]],
) -> Result<(), TessellationError> {
    if feature_edges.iter().any(|edge| {
        edge[0] >= edge[1] || usize::try_from(edge[1]).map_or(true, |index| index >= vertex_count)
    }) || feature_edges.windows(2).any(|edges| edges[0] >= edges[1])
    {
        return Err(tessellation_error(
            "contains an invalid tessellation feature edge",
        ));
    }
    Ok(())
}

fn require_triangle_groups(
    triangle_count: usize,
    triangle_groups: &[TessellationTriangleGroup],
) -> Result<(), TessellationError> {
    if triangle_groups.is_empty() {
        return Ok(());
    }
    let mut memberships = std::iter::repeat_n(false, triangle_count).collect::<Vec<_>>();
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
    triangle_count: usize,
    texture_assignments: &[TessellationTextureAssignment],
) -> Result<(), TessellationError> {
    if texture_assignments.is_empty() {
        return Ok(());
    }
    let mut memberships = std::iter::repeat_n(false, triangle_count).collect::<Vec<_>>();
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
    /// Build a tessellation from its mesh rows and channels.
    pub fn new(
        id: impl Into<String>,
        mesh: TessellationMesh,
        channels: Vec<TessellationChannel>,
    ) -> Result<Self, TessellationError> {
        let vertices = mesh.vertices();
        if vertices
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
        {
            return Err(tessellation_error(
                "vertices contain a non-finite coordinate",
            ));
        }
        require_finite_normals(&mesh.vertex_normals())?;
        require_finite_normals(&mesh.corner_normals())?;
        let triangles = mesh.triangles();
        require_triangle_indices(vertices.len(), &triangles)?;
        require_channel_indices(triangles.len(), &channels)?;
        Ok(Self {
            id: TessellationId::mint(id).map_err(|error| tessellation_error(error.to_string()))?,
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            mesh,
            feature_edges: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels,
        })
    }

    /// The mesh's vertices, triangles and shading normals.
    #[must_use]
    pub const fn mesh(&self) -> &TessellationMesh {
        &self.mesh
    }

    /// Vertex positions in document units.
    #[must_use]
    pub fn vertices(&self) -> Vec<Point3> {
        self.mesh.vertices()
    }

    /// Number of vertices in the mesh.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.mesh.vertex_count()
    }

    /// Atomically edit vertex positions while preserving finite coordinates.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnMut(&mut Point3),
    ) -> Result<(), TessellationError> {
        let mut mesh = self.mesh.clone();
        mesh.edit_positions(edit);
        require_finite_vertices(&mesh.vertices())?;
        self.mesh = mesh;
        Ok(())
    }

    /// Zero-based vertex indices, with source winding preserved.
    ///
    /// A strip mesh derives its triangles from the strips it owns.
    #[must_use]
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        self.mesh.triangles()
    }

    /// Number of triangles in the mesh.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.mesh.triangle_count()
    }

    /// Strip spans in mesh order; empty when the mesh is a flat triangle list.
    #[must_use]
    pub fn strip_lengths(&self) -> Vec<u32> {
        self.mesh.strip_lengths()
    }

    /// Per-vertex normals; empty when the source carried none or corner normals.
    #[must_use]
    pub fn vertex_normals(&self) -> Vec<Vector3> {
        self.mesh.vertex_normals()
    }

    /// Atomically edit the stored shading normals while preserving finite coordinates.
    pub fn edit_normals(
        &mut self,
        edit: impl FnMut(&mut Vector3),
    ) -> Result<(), TessellationError> {
        let mut mesh = self.mesh.clone();
        if !mesh.edit_normals(edit) {
            return Err(tessellation_error("mesh has no shading normals to edit"));
        }
        require_finite_normals(&mesh.vertex_normals())?;
        require_finite_normals(&mesh.corner_normals())?;
        self.mesh = mesh;
        Ok(())
    }

    /// Per-triangle-corner normals; empty when the source carried none or vertex normals.
    #[must_use]
    pub fn per_corner_normals(&self) -> Vec<Vector3> {
        self.mesh.corner_normals()
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
        require_feature_edges(self.mesh.vertex_count(), &feature_edges)?;
        self.feature_edges = feature_edges;
        Ok(self)
    }

    /// Set the triangle-group partition.
    pub fn with_triangle_groups(
        mut self,
        triangle_groups: Vec<TessellationTriangleGroup>,
    ) -> Result<Self, TessellationError> {
        require_triangle_groups(self.mesh.triangle_count(), &triangle_groups)?;
        self.triangle_groups = triangle_groups;
        Ok(self)
    }

    /// Set the texture assignments.
    pub fn with_texture_assignments(
        mut self,
        texture_assignments: Vec<TessellationTextureAssignment>,
    ) -> Result<Self, TessellationError> {
        require_texture_assignments(self.mesh.triangle_count(), &texture_assignments)?;
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
    fn from(tessellation: Tessellation) -> Self {
        Self {
            id: tessellation.id,
            body: tessellation.body,
            faces: tessellation.faces,
            chordal_deflection: tessellation.chordal_deflection,
            source_object: tessellation.source_object,
            mesh: tessellation.mesh,
            feature_edges: tessellation.feature_edges,
            triangle_groups: tessellation.triangle_groups,
            texture_assignments: tessellation.texture_assignments,
            channels: tessellation.channels,
        }
    }
}

impl TryFrom<TessellationWire> for Tessellation {
    type Error = TessellationError;

    fn try_from(wire: TessellationWire) -> Result<Self, Self::Error> {
        let mut mesh = Self::new(wire.id.into_string(), wire.mesh, wire.channels)?;
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
        Self {
            addressing: channel.addressing,
            item_size: channel.item_size,
            kind: channel.kind,
            flags: channel.flags,
            data: channel.data,
        }
    }
}

impl TryFrom<TessellationChannelWire> for TessellationChannel {
    type Error = TessellationError;

    fn try_from(wire: TessellationChannelWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.addressing,
            wire.item_size,
            wire.kind,
            wire.flags,
            wire.data,
        )
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
