// SPDX-License-Identifier: Apache-2.0
//! Subdivision-surface control cages.

use crate::ids::SubdId;
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A subdivision surface represented by its control cage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdSurface {
    /// Arena identity.
    pub id: SubdId,
    /// Subdivision scheme.
    pub scheme: SubdScheme,
    /// Control cage with admitted local topology and payloads.
    #[serde(flatten)]
    pub cage: SubdCage,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
}

/// Admission error in a subdivision control cage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubdError(String);

impl std::fmt::Display for SubdError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SubdError {}

const EPS_SUBD_SYMMETRY_FRAME: f64 = 1.0e-9;

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

/// A subdivision cage with valid local topology and numeric payloads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdCageWire")]
pub struct SubdCage {
    vertices: Vec<SubdVertex>,
    edges: Vec<SubdEdge>,
    faces: Vec<SubdFace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    symmetries: Vec<SubdSymmetry>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdCageWire {
    vertices: Vec<SubdVertex>,
    edges: Vec<SubdEdge>,
    faces: Vec<SubdFace>,
    #[serde(default)]
    symmetries: Vec<SubdSymmetry>,
}

impl TryFrom<SubdCageWire> for SubdCage {
    type Error = SubdError;

    fn try_from(wire: SubdCageWire) -> Result<Self, Self::Error> {
        Self::new(wire.vertices, wire.edges, wire.faces, wire.symmetries)
    }
}

impl SubdCage {
    /// Construct a cage with closed directed rings and valid local payloads.
    pub fn new(
        vertices: Vec<SubdVertex>,
        edges: Vec<SubdEdge>,
        faces: Vec<SubdFace>,
        symmetries: Vec<SubdSymmetry>,
    ) -> Result<Self, SubdError> {
        let cage = Self {
            vertices,
            edges,
            faces,
            symmetries,
        };
        cage.validate()?;
        Ok(cage)
    }

    /// Control vertices in cage order.
    pub fn vertices(&self) -> &[SubdVertex] {
        &self.vertices
    }

    /// Control edges in cage order.
    pub fn edges(&self) -> &[SubdEdge] {
        &self.edges
    }

    /// Control faces in cage order.
    pub fn faces(&self) -> &[SubdFace] {
        &self.faces
    }

    /// Editor symmetry blocks in cage order.
    pub fn symmetries(&self) -> &[SubdSymmetry] {
        &self.symmetries
    }

    /// Atomically edit vertices and their grip layouts while preserving cage invariants.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnOnce(&mut [SubdVertex]) -> Result<(), SubdError>,
    ) -> Result<(), SubdError> {
        let mut vertices = self.vertices.clone();
        edit(&mut vertices)?;
        self.validate_vertices(&vertices)?;
        self.vertices = vertices;
        Ok(())
    }

    fn validate(&self) -> Result<(), SubdError> {
        for (index, edge) in self.edges.iter().enumerate() {
            if edge.vertices[0] == edge.vertices[1]
                || edge
                    .vertices
                    .iter()
                    .any(|vertex| *vertex as usize >= self.vertices.len())
                || edge
                    .sharpness
                    .iter()
                    .any(|value| !value.is_finite() || *value < 0.0)
                || edge
                    .knot_interval
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
                || edge
                    .sector_coefficients
                    .iter()
                    .any(|value| !value.is_finite())
            {
                return Err(SubdError(format!(
                    "edges[{index}] has invalid endpoints or numeric payloads"
                )));
            }
        }
        for (index, face) in self.faces.iter().enumerate() {
            let endpoints = face
                .edges
                .iter()
                .map(|use_| {
                    let edge = self.edges.get(use_.edge as usize).ok_or_else(|| {
                        SubdError(format!("faces[{index}].edges references a missing edge"))
                    })?;
                    Ok(if use_.reversed {
                        [edge.vertices[1], edge.vertices[0]]
                    } else {
                        edge.vertices
                    })
                })
                .collect::<Result<Vec<_>, SubdError>>()?;
            if endpoints
                .iter()
                .zip(endpoints.iter().cycle().skip(1))
                .any(|(first, next)| first[1] != next[0])
            {
                return Err(SubdError(format!(
                    "faces[{index}].edges is not a directed closed ring"
                )));
            }
        }
        self.validate_vertices(&self.vertices)?;
        for symmetry in &self.symmetries {
            symmetry.validate()?;
            for (field, pairs, count) in [
                ("face_pairs", &symmetry.face_pairs, self.faces.len()),
                ("edge_pairs", &symmetry.edge_pairs, self.edges.len()),
                ("vertex_pairs", &symmetry.vertex_pairs, self.vertices.len()),
            ] {
                if pairs.iter().flatten().any(|index| *index as usize >= count) {
                    return Err(SubdError(format!(
                        "symmetries.{field} contains an out-of-range index"
                    )));
                }
            }
        }
        Ok(())
    }
    fn validate_vertices(&self, vertices: &[SubdVertex]) -> Result<(), SubdError> {
        let mut grip_indices = std::collections::BTreeSet::new();
        for (index, vertex) in vertices.iter().enumerate() {
            if !finite_point(vertex.point) {
                return Err(SubdError(format!("vertices[{index}].point is not finite")));
            }
            let Some(layout) = &vertex.secondary_grips else {
                continue;
            };
            if layout.wedges.is_empty() {
                return Err(SubdError(format!(
                    "vertices[{index}].secondary_grips.wedges is empty"
                )));
            }
            for (wedge_index, wedge) in layout.wedges.iter().enumerate() {
                let spoke_count = |wedge: &SubdGripWedge| match wedge {
                    SubdGripWedge::Phantom => 0,
                    SubdGripWedge::Slot { spokes, .. } => spokes.len(),
                };
                let next = &layout.wedges[(wedge_index + 1) % layout.wedges.len()];
                let sector_count = match wedge {
                    SubdGripWedge::Phantom => 0,
                    SubdGripWedge::Slot { sectors, .. } => sectors.len(),
                };
                if spoke_count(wedge).checked_mul(spoke_count(next)) != Some(sector_count) {
                    return Err(SubdError(format!("vertices[{index}].secondary_grips.wedges[{wedge_index}].sectors has invalid arity")));
                }
                let SubdGripWedge::Slot {
                    edge,
                    sector_face,
                    spokes,
                    sectors,
                } = wedge
                else {
                    continue;
                };
                if let Some(edge) = edge {
                    let edge = self.edges.get(*edge as usize).ok_or_else(|| {
                        SubdError(format!(
                            "vertices[{index}].secondary_grips edge is out of range"
                        ))
                    })?;
                    if !edge.vertices.iter().any(|owner| *owner as usize == index) {
                        return Err(SubdError(format!(
                            "vertices[{index}].secondary_grips edge is not incident to its owner"
                        )));
                    }
                }
                if let Some(face) = sector_face {
                    let face = self.faces.get(*face as usize).ok_or_else(|| {
                        SubdError(format!(
                            "vertices[{index}].secondary_grips sector_face is out of range"
                        ))
                    })?;
                    if !face.edges.iter().any(|use_| {
                        self.edges[use_.edge as usize]
                            .vertices
                            .iter()
                            .any(|owner| *owner as usize == index)
                    }) {
                        return Err(SubdError(format!("vertices[{index}].secondary_grips sector_face is not incident to its owner")));
                    }
                }
                for grip in spokes.iter().chain(sectors).flatten() {
                    if !finite_point(grip.point)
                        || !grip.weight.is_finite()
                        || grip.weight <= 0.0
                        || !grip_indices.insert(grip.source_index)
                    {
                        return Err(SubdError(format!(
                            "vertices[{index}].secondary_grips has an invalid or repeated grip"
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

/// A symmetry plane frame carried by a T-spline editor block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdPlaneFrame {
    /// A point on the plane in document length units.
    pub origin: Point3,
    /// First unit in-plane axis.
    pub first_axis: Vector3,
    /// Second unit in-plane axis.
    pub second_axis: Vector3,
}

/// Kind-specific controls for a T-spline symmetry block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubdSymmetryKind {
    /// One-to-one correspondence across the symmetry plane.
    Correspondence,
    /// Radial editor symmetry with native segment and sweep controls.
    Radial {
        /// Number of radial segments.
        segments: u32,
        /// Native radial sweep value.
        sweep: f64,
        /// Selector-preserving native radial-symmetry maps.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        radial_maps: Vec<SubdRadialSymmetryMap>,
    },
}

/// Selector of one native radial-symmetry map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdRadialMapSelector {
    /// Native `ef` map.
    Ef,
    /// Native `er` map.
    Er,
    /// Native `ff` map.
    Ff,
    /// Native `fr` map.
    Fr,
    /// Native `vf` map.
    Vf,
    /// Native `vr` map.
    Vr,
}

/// One selector-preserving native radial-symmetry map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdRadialSymmetryMap {
    /// Native map selector. Its element namespace is format-native.
    pub selector: SubdRadialMapSelector,
    /// Native source/target identifier pairs.
    pub pairs: Vec<[u64; 2]>,
}

/// Typed editor symmetry state for one subdivision cage.
#[derive(Debug, Clone, PartialEq)]
pub struct SubdSymmetry {
    /// Symmetry mode and its radial controls, when present.
    pub kind: SubdSymmetryKind,
    /// Geometric symmetry-plane frame.
    pub plane: SubdPlaneFrame,
    /// Forward face correspondences for a topology-addressed symmetry block.
    pub face_pairs: Vec<[u32; 2]>,
    /// Forward edge correspondences for a topology-addressed symmetry block.
    pub edge_pairs: Vec<[u32; 2]>,
    /// Forward vertex correspondences for a topology-addressed symmetry block.
    pub vertex_pairs: Vec<[u32; 2]>,
}

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SubdSymmetryKindWire {
    Correspondence,
    Radial { segments: u32, sweep: f64 },
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdSymmetryWire {
    kind: SubdSymmetryKindWire,
    plane: SubdPlaneFrame,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    face_pairs: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edge_pairs: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_pairs: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    radial_maps: Vec<SubdRadialSymmetryMap>,
}

impl SubdSymmetry {
    fn validate(&self) -> Result<(), SubdError> {
        let plane = &self.plane;
        if !finite_point(plane.origin)
            || !finite_vector(plane.first_axis)
            || !finite_vector(plane.second_axis)
            || (plane.first_axis.norm() - 1.0).abs() > EPS_SUBD_SYMMETRY_FRAME
            || (plane.second_axis.norm() - 1.0).abs() > EPS_SUBD_SYMMETRY_FRAME
            || plane.first_axis.dot(plane.second_axis).abs() > EPS_SUBD_SYMMETRY_FRAME
        {
            return Err(SubdError("plane has an invalid orthonormal frame".into()));
        }
        if let SubdSymmetryKind::Radial {
            segments,
            sweep,
            radial_maps,
        } = &self.kind
        {
            if *segments == 0 || !sweep.is_finite() {
                return Err(SubdError(
                    "kind.radial requires nonzero segments and finite sweep".into(),
                ));
            }
            let mut selectors = std::collections::BTreeSet::new();
            for map in radial_maps {
                if !selectors.insert(map.selector) {
                    return Err(SubdError("radial_maps repeats a selector".into()));
                }
                let mut sources = std::collections::BTreeSet::new();
                if map.pairs.iter().any(|[source, _]| !sources.insert(*source)) {
                    return Err(SubdError("radial_maps.pairs repeats a source".into()));
                }
            }
        }
        for (field, pairs) in [
            ("face_pairs", &self.face_pairs),
            ("edge_pairs", &self.edge_pairs),
            ("vertex_pairs", &self.vertex_pairs),
        ] {
            let mut sources = std::collections::BTreeSet::new();
            let mut targets = std::collections::BTreeSet::new();
            if pairs
                .iter()
                .any(|[source, target]| !sources.insert(*source) || !targets.insert(*target))
            {
                return Err(SubdError(format!("{field} repeats a source or target")));
            }
        }
        Ok(())
    }
}

impl Serialize for SubdSymmetry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, radial_maps) = match &self.kind {
            SubdSymmetryKind::Correspondence => (SubdSymmetryKindWire::Correspondence, Vec::new()),
            SubdSymmetryKind::Radial {
                segments,
                sweep,
                radial_maps,
            } => (
                SubdSymmetryKindWire::Radial {
                    segments: *segments,
                    sweep: *sweep,
                },
                radial_maps.clone(),
            ),
        };
        SubdSymmetryWire {
            kind,
            plane: self.plane,
            face_pairs: self.face_pairs.clone(),
            edge_pairs: self.edge_pairs.clone(),
            vertex_pairs: self.vertex_pairs.clone(),
            radial_maps,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SubdSymmetry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SubdSymmetryWire::deserialize(deserializer)?;
        let kind = match wire.kind {
            SubdSymmetryKindWire::Correspondence if wire.radial_maps.is_empty() => {
                SubdSymmetryKind::Correspondence
            }
            SubdSymmetryKindWire::Correspondence => {
                return Err(serde::de::Error::custom(
                    "correspondence SubD symmetry cannot carry radial_maps",
                ));
            }
            SubdSymmetryKindWire::Radial { segments, sweep } => SubdSymmetryKind::Radial {
                segments,
                sweep,
                radial_maps: wire.radial_maps,
            },
        };
        Ok(Self {
            kind,
            plane: wire.plane,
            face_pairs: wire.face_pairs,
            edge_pairs: wire.edge_pairs,
            vertex_pairs: wire.vertex_pairs,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SubdSymmetry {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SubdSymmetry".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SubdSymmetryWire::json_schema(generator)
    }
}

/// Subdivision scheme used by a control cage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdScheme {
    /// Catmull-Clark subdivision.
    CatmullClark,
}

/// A control-cage vertex and its subdivision tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdVertex {
    /// Vertex position.
    pub point: Point3,
    /// Subdivision vertex tag.
    pub tag: SubdVertexTag,
    /// Optional secondary-grip topology owned by this vertex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_grips: Option<SubdVertexGripLayout>,
}

/// Compass direction of the root edge in a control-cage grid frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdGripDirection {
    /// Positive grid-y direction.
    North,
    /// Positive grid-x direction.
    East,
    /// Negative grid-y direction.
    South,
    /// Negative grid-x direction.
    West,
}

/// Typed secondary-grip layout for one subdivision vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdVertexGripLayout {
    /// Direction of the native root edge; wedge zero is the north slot.
    pub direction: SubdGripDirection,
    /// Wedges in north-anchored order.
    pub wedges: Vec<SubdGripWedge>,
}

/// One wedge in a secondary-grip layout.
#[derive(Debug, Clone, PartialEq)]
pub enum SubdGripWedge {
    /// Boundary padding with no topology or grip data.
    Phantom,
    /// One topology and grip slot in the vertex fan.
    Slot {
        /// IR edge for this fan slot.
        edge: Option<u32>,
        /// Face in the sector following this slot, or `None` for a boundary gap.
        sector_face: Option<u32>,
        /// Spoke grips ordered nearest-first from the owning vertex.
        spokes: Vec<Option<SubdSecondaryGrip>>,
        /// Sector-grid grips ordered by the spoke-k position, then the
        /// next-spoke position, with `S[k] * S[k + 1]` slots.
        sectors: Vec<Option<SubdSecondaryGrip>>,
    },
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdGripWedgeWire {
    edge: Option<u32>,
    sector_face: Option<u32>,
    phantom: bool,
    spokes: Vec<Option<SubdSecondaryGrip>>,
    sectors: Vec<Option<SubdSecondaryGrip>>,
}

impl Serialize for SubdGripWedge {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let wire = match self {
            Self::Phantom => SubdGripWedgeWire {
                edge: None,
                sector_face: None,
                phantom: true,
                spokes: Vec::new(),
                sectors: Vec::new(),
            },
            Self::Slot {
                edge,
                sector_face,
                spokes,
                sectors,
            } => SubdGripWedgeWire {
                edge: *edge,
                sector_face: *sector_face,
                phantom: false,
                spokes: spokes.clone(),
                sectors: sectors.clone(),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SubdGripWedge {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SubdGripWedgeWire::deserialize(deserializer)?;
        if !wire.phantom {
            return Ok(Self::Slot {
                edge: wire.edge,
                sector_face: wire.sector_face,
                spokes: wire.spokes,
                sectors: wire.sectors,
            });
        }
        if wire.edge.is_some()
            || wire.sector_face.is_some()
            || !wire.spokes.is_empty()
            || !wire.sectors.is_empty()
        {
            return Err(serde::de::Error::custom(
                "phantom SubD grip wedge cannot carry topology or grip data",
            ));
        }
        Ok(Self::Phantom)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SubdGripWedge {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SubdGripWedge".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SubdGripWedgeWire::json_schema(generator)
    }
}

/// A secondary grip point and its source grip-array identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdSecondaryGrip {
    /// Index in the source cage's `0g` grip array.
    pub source_index: u32,
    /// Grip position in document units.
    pub point: Point3,
    /// Positive rational grip weight.
    pub weight: f64,
}

/// A control-cage vertex tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdVertexTag {
    /// Smooth vertex.
    Smooth,
    /// Crease vertex.
    Crease,
    /// Corner vertex.
    Corner,
    /// Dart vertex.
    Dart,
}

/// A control-cage edge with endpoint sharpness and sector coefficients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdEdge {
    /// Indices of the two distinct endpoint vertices.
    pub vertices: [u32; 2],
    /// Sharpness at the start and end endpoints.
    pub sharpness: [f64; 2],
    /// Subdivision edge tag.
    pub tag: SubdEdgeTag,
    /// Parametric knot interval, when the source cage exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knot_interval: Option<f64>,
    /// Sector coefficients at the two endpoints.
    pub sector_coefficients: [f64; 2],
}

/// A control-cage edge tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubdEdgeTag {
    /// Smooth edge.
    Smooth,
    /// Smooth-X edge with the source's distinct subdivision behavior.
    SmoothX,
    /// Crease edge.
    Crease,
}

/// A subdivision face bounded by a directed edge ring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdFaceWire")]
pub struct SubdFace {
    edges: Vec<SubdEdgeUse>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdFaceWire {
    edges: Vec<SubdEdgeUse>,
}

impl TryFrom<SubdFaceWire> for SubdFace {
    type Error = SubdError;

    fn try_from(wire: SubdFaceWire) -> Result<Self, Self::Error> {
        Self::new(wire.edges)
    }
}

impl SubdFace {
    /// Construct a face with at least three directed edge uses.
    pub fn new(edges: Vec<SubdEdgeUse>) -> Result<Self, SubdError> {
        if edges.len() < 3 {
            return Err(SubdError("edges must contain at least three uses".into()));
        }
        Ok(Self { edges })
    }

    /// Directed edge uses in boundary order.
    pub fn edges(&self) -> &[SubdEdgeUse] {
        &self.edges
    }
}

/// One directed use of a subdivision edge in a face ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SubdEdgeUse {
    /// Index into the parent surface's edge array.
    pub edge: u32,
    /// Whether this use traverses the edge from its second endpoint.
    pub reversed: bool,
}

#[cfg(test)]
mod tests;
