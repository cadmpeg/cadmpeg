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

fn require_finite_point(field: &str, point: Point3) -> Result<(), SubdError> {
    if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
        return Err(SubdError(format!("{field} must be finite")));
    }
    Ok(())
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
            if edge
                .vertices
                .iter()
                .any(|vertex| *vertex as usize >= self.vertices.len())
            {
                return Err(SubdError(format!(
                    "edges[{index}].vertices contains an out-of-range index"
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
            let Some(layout) = &vertex.secondary_grips else {
                continue;
            };
            for wedge in &layout.wedges {
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
                    if !grip_indices.insert(grip.source_index) {
                        return Err(SubdError(format!(
                            "vertices[{index}].secondary_grips repeats a source_index"
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
#[serde(try_from = "SubdPlaneFrameWire")]
pub struct SubdPlaneFrame {
    /// A point on the plane in document length units.
    origin: Point3,
    /// First unit in-plane axis.
    first_axis: Vector3,
    /// Second unit in-plane axis.
    second_axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdPlaneFrameWire {
    origin: Point3,
    first_axis: Vector3,
    second_axis: Vector3,
}

impl TryFrom<SubdPlaneFrameWire> for SubdPlaneFrame {
    type Error = SubdError;

    fn try_from(wire: SubdPlaneFrameWire) -> Result<Self, Self::Error> {
        Self::new(wire.origin, wire.first_axis, wire.second_axis)
    }
}

impl SubdPlaneFrame {
    /// Construct a finite plane frame with orthonormal axes.
    pub fn new(
        origin: Point3,
        first_axis: Vector3,
        second_axis: Vector3,
    ) -> Result<Self, SubdError> {
        require_finite_point("origin", origin)?;
        if !finite_vector(first_axis) || (first_axis.norm() - 1.0).abs() > EPS_SUBD_SYMMETRY_FRAME {
            return Err(SubdError(
                "first_axis must be finite and unit length".into(),
            ));
        }
        if !finite_vector(second_axis) || (second_axis.norm() - 1.0).abs() > EPS_SUBD_SYMMETRY_FRAME
        {
            return Err(SubdError(
                "second_axis must be finite and unit length".into(),
            ));
        }
        if first_axis.dot(second_axis).abs() > EPS_SUBD_SYMMETRY_FRAME {
            return Err(SubdError(
                "first_axis and second_axis must be orthogonal".into(),
            ));
        }
        Ok(Self {
            origin,
            first_axis,
            second_axis,
        })
    }

    /// A point on the symmetry plane in document units.
    pub const fn origin(&self) -> Point3 {
        self.origin
    }

    /// First unit in-plane axis.
    pub const fn first_axis(&self) -> Vector3 {
        self.first_axis
    }

    /// Second unit in-plane axis.
    pub const fn second_axis(&self) -> Vector3 {
        self.second_axis
    }
}

/// Kind-specific controls for a T-spline symmetry block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubdSymmetryKind {
    /// One-to-one correspondence across the symmetry plane.
    Correspondence,
    /// Radial editor symmetry with native segment and sweep controls.
    Radial(SubdRadialSymmetry),
}

/// Admitted radial controls and selector-preserving maps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdRadialSymmetryWire")]
pub struct SubdRadialSymmetry {
    segments: std::num::NonZeroU32,
    sweep: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    radial_maps: Vec<SubdRadialSymmetryMap>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdRadialSymmetryWire {
    segments: std::num::NonZeroU32,
    sweep: f64,
    #[serde(default)]
    radial_maps: Vec<SubdRadialSymmetryMap>,
}

impl TryFrom<SubdRadialSymmetryWire> for SubdRadialSymmetry {
    type Error = SubdError;

    fn try_from(wire: SubdRadialSymmetryWire) -> Result<Self, Self::Error> {
        Self::new(wire.segments, wire.sweep, wire.radial_maps)
    }
}

impl SubdRadialSymmetry {
    fn new(
        segments: std::num::NonZeroU32,
        sweep: f64,
        radial_maps: Vec<SubdRadialSymmetryMap>,
    ) -> Result<Self, SubdError> {
        if !sweep.is_finite() {
            return Err(SubdError("kind.radial.sweep must be finite".into()));
        }
        let mut selectors = std::collections::BTreeSet::new();
        for map in &radial_maps {
            if !selectors.insert(map.selector) {
                return Err(SubdError("radial_maps repeats a selector".into()));
            }
            let mut sources = std::collections::BTreeSet::new();
            if map.pairs.iter().any(|[source, _]| !sources.insert(*source)) {
                return Err(SubdError("radial_maps.pairs repeats a source".into()));
            }
        }
        Ok(Self {
            segments,
            sweep,
            radial_maps,
        })
    }

    /// Number of radial segments.
    pub const fn segments(&self) -> std::num::NonZeroU32 {
        self.segments
    }

    /// Finite native radial sweep.
    pub const fn sweep(&self) -> f64 {
        self.sweep
    }

    /// Native maps with distinct selectors and distinct sources within each map.
    pub fn radial_maps(&self) -> &[SubdRadialSymmetryMap] {
        &self.radial_maps
    }
}

impl SubdSymmetryKind {
    /// Admit radial controls with finite sweep and distinct map selectors and sources.
    pub fn radial(
        segments: std::num::NonZeroU32,
        sweep: f64,
        radial_maps: Vec<SubdRadialSymmetryMap>,
    ) -> Result<Self, SubdError> {
        SubdRadialSymmetry::new(segments, sweep, radial_maps).map(Self::Radial)
    }
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
    kind: SubdSymmetryKind,
    /// Geometric symmetry-plane frame.
    pub plane: SubdPlaneFrame,
    /// Forward face correspondences for a topology-addressed symmetry block.
    face_pairs: Vec<[u32; 2]>,
    /// Forward edge correspondences for a topology-addressed symmetry block.
    edge_pairs: Vec<[u32; 2]>,
    /// Forward vertex correspondences for a topology-addressed symmetry block.
    vertex_pairs: Vec<[u32; 2]>,
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
    /// Construct symmetry state with a finite radial sweep and distinct correspondences.
    pub fn new(
        kind: SubdSymmetryKind,
        plane: SubdPlaneFrame,
        face_pairs: Vec<[u32; 2]>,
        edge_pairs: Vec<[u32; 2]>,
        vertex_pairs: Vec<[u32; 2]>,
    ) -> Result<Self, SubdError> {
        for (field, pairs) in [
            ("face_pairs", &face_pairs),
            ("edge_pairs", &edge_pairs),
            ("vertex_pairs", &vertex_pairs),
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
        Ok(Self {
            kind,
            plane,
            face_pairs,
            edge_pairs,
            vertex_pairs,
        })
    }

    /// Symmetry mode and its radial controls.
    pub fn kind(&self) -> &SubdSymmetryKind {
        &self.kind
    }

    /// Forward face correspondences.
    pub fn face_pairs(&self) -> &[[u32; 2]] {
        &self.face_pairs
    }

    /// Forward edge correspondences.
    pub fn edge_pairs(&self) -> &[[u32; 2]] {
        &self.edge_pairs
    }

    /// Forward vertex correspondences.
    pub fn vertex_pairs(&self) -> &[[u32; 2]] {
        &self.vertex_pairs
    }
}

impl Serialize for SubdSymmetry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, radial_maps) = match &self.kind {
            SubdSymmetryKind::Correspondence => (SubdSymmetryKindWire::Correspondence, Vec::new()),
            SubdSymmetryKind::Radial(radial) => (
                SubdSymmetryKindWire::Radial {
                    segments: radial.segments().get(),
                    sweep: radial.sweep(),
                },
                radial.radial_maps().to_vec(),
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
            SubdSymmetryKindWire::Radial { segments, sweep } => SubdSymmetryKind::radial(
                std::num::NonZeroU32::new(segments).ok_or_else(|| {
                    serde::de::Error::custom("kind.radial.segments must be nonzero")
                })?,
                sweep,
                wire.radial_maps,
            )
            .map_err(serde::de::Error::custom)?,
        };
        Self::new(
            kind,
            wire.plane,
            wire.face_pairs,
            wire.edge_pairs,
            wire.vertex_pairs,
        )
        .map_err(serde::de::Error::custom)
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
#[serde(try_from = "SubdVertexWire")]
pub struct SubdVertex {
    /// Vertex position.
    point: Point3,
    /// Subdivision vertex tag.
    pub tag: SubdVertexTag,
    /// Optional secondary-grip topology owned by this vertex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_grips: Option<SubdVertexGripLayout>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdVertexWire {
    point: Point3,
    tag: SubdVertexTag,
    #[serde(default)]
    secondary_grips: Option<SubdVertexGripLayout>,
}

impl TryFrom<SubdVertexWire> for SubdVertex {
    type Error = SubdError;

    fn try_from(wire: SubdVertexWire) -> Result<Self, Self::Error> {
        Self::new(wire.point, wire.tag, wire.secondary_grips)
    }
}

impl SubdVertex {
    /// Construct a control vertex with a finite position.
    pub fn new(
        point: Point3,
        tag: SubdVertexTag,
        secondary_grips: Option<SubdVertexGripLayout>,
    ) -> Result<Self, SubdError> {
        require_finite_point("point", point)?;
        Ok(Self {
            point,
            tag,
            secondary_grips,
        })
    }

    /// Optional admitted secondary-grip layout.
    pub fn secondary_grips(&self) -> Option<&SubdVertexGripLayout> {
        self.secondary_grips.as_ref()
    }

    /// Vertex position in document units.
    pub const fn point(&self) -> Point3 {
        self.point
    }

    /// Replace the vertex position with finite coordinates.
    pub fn set_point(&mut self, point: Point3) -> Result<(), SubdError> {
        require_finite_point("point", point)?;
        self.point = point;
        Ok(())
    }
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
#[serde(try_from = "SubdVertexGripLayoutWire")]
pub struct SubdVertexGripLayout {
    /// Direction of the native root edge; wedge zero is the north slot.
    direction: SubdGripDirection,
    /// Wedges in north-anchored order.
    wedges: Vec<SubdGripWedge>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdVertexGripLayoutWire {
    direction: SubdGripDirection,
    wedges: Vec<SubdGripWedge>,
}

impl TryFrom<SubdVertexGripLayoutWire> for SubdVertexGripLayout {
    type Error = SubdError;

    fn try_from(wire: SubdVertexGripLayoutWire) -> Result<Self, Self::Error> {
        Self::new(wire.direction, wire.wedges)
    }
}

impl SubdVertexGripLayout {
    /// Admit a nonempty cyclic fan with sector arity equal to adjacent spoke products.
    pub fn new(
        direction: SubdGripDirection,
        wedges: Vec<SubdGripWedge>,
    ) -> Result<Self, SubdError> {
        if wedges.is_empty() {
            return Err(SubdError("secondary_grips.wedges is empty".into()));
        }
        let spoke_count = |wedge: &SubdGripWedge| match wedge {
            SubdGripWedge::Phantom => 0,
            SubdGripWedge::Slot { spokes, .. } => spokes.len(),
        };
        for (index, (wedge, next)) in wedges.iter().zip(wedges.iter().cycle().skip(1)).enumerate() {
            let sector_count = match wedge {
                SubdGripWedge::Phantom => 0,
                SubdGripWedge::Slot { sectors, .. } => sectors.len(),
            };
            if spoke_count(wedge).checked_mul(spoke_count(next)) != Some(sector_count) {
                return Err(SubdError(format!(
                    "secondary_grips.wedges[{index}].sectors has invalid arity"
                )));
            }
        }
        Ok(Self { direction, wedges })
    }

    /// Direction of the native root edge.
    pub const fn direction(&self) -> SubdGripDirection {
        self.direction
    }

    /// Wedges in north-anchored cyclic order.
    pub fn wedges(&self) -> &[SubdGripWedge] {
        &self.wedges
    }
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
#[serde(try_from = "SubdSecondaryGripWire")]
pub struct SubdSecondaryGrip {
    /// Index in the source cage's `0g` grip array.
    pub source_index: u32,
    /// Grip position in document units.
    point: Point3,
    /// Positive rational grip weight.
    weight: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdSecondaryGripWire {
    source_index: u32,
    point: Point3,
    weight: f64,
}

impl TryFrom<SubdSecondaryGripWire> for SubdSecondaryGrip {
    type Error = SubdError;

    fn try_from(wire: SubdSecondaryGripWire) -> Result<Self, Self::Error> {
        Self::new(wire.source_index, wire.point, wire.weight)
    }
}

impl SubdSecondaryGrip {
    /// Construct a finite grip point with a positive finite rational weight.
    pub fn new(source_index: u32, point: Point3, weight: f64) -> Result<Self, SubdError> {
        require_finite_point("point", point)?;
        if !weight.is_finite() || weight <= 0.0 {
            return Err(SubdError("weight must be finite and positive".into()));
        }
        Ok(Self {
            source_index,
            point,
            weight,
        })
    }

    /// Grip position in document units.
    pub const fn point(&self) -> Point3 {
        self.point
    }

    /// Positive rational grip weight.
    pub const fn weight(&self) -> f64 {
        self.weight
    }
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
#[serde(try_from = "SubdEdgeWire")]
pub struct SubdEdge {
    /// Indices of the two distinct endpoint vertices.
    vertices: [u32; 2],
    /// Sharpness at the start and end endpoints.
    sharpness: [f64; 2],
    /// Subdivision edge tag.
    pub tag: SubdEdgeTag,
    /// Parametric knot interval, when the source cage exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    knot_interval: Option<f64>,
    /// Sector coefficients at the two endpoints.
    sector_coefficients: [f64; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SubdEdgeWire {
    vertices: [u32; 2],
    sharpness: [f64; 2],
    tag: SubdEdgeTag,
    #[serde(default)]
    knot_interval: Option<f64>,
    sector_coefficients: [f64; 2],
}

impl TryFrom<SubdEdgeWire> for SubdEdge {
    type Error = SubdError;

    fn try_from(wire: SubdEdgeWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.vertices,
            wire.sharpness,
            wire.tag,
            wire.knot_interval,
            wire.sector_coefficients,
        )
    }
}

impl SubdEdge {
    /// Construct an edge with distinct endpoints and admitted numeric controls.
    pub fn new(
        vertices: [u32; 2],
        sharpness: [f64; 2],
        tag: SubdEdgeTag,
        knot_interval: Option<f64>,
        sector_coefficients: [f64; 2],
    ) -> Result<Self, SubdError> {
        if vertices[0] == vertices[1] {
            return Err(SubdError("vertices must name distinct endpoints".into()));
        }
        if sharpness
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(SubdError(
                "sharpness must be finite and non-negative".into(),
            ));
        }
        if knot_interval.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(SubdError(
                "knot_interval must be finite and positive".into(),
            ));
        }
        if sector_coefficients.iter().any(|value| !value.is_finite()) {
            return Err(SubdError("sector_coefficients must be finite".into()));
        }
        Ok(Self {
            vertices,
            sharpness,
            tag,
            knot_interval,
            sector_coefficients,
        })
    }

    /// Indices of the two distinct endpoint vertices.
    pub const fn vertices(&self) -> [u32; 2] {
        self.vertices
    }

    /// Sharpness at the two endpoints.
    pub const fn sharpness(&self) -> [f64; 2] {
        self.sharpness
    }

    /// Parametric knot interval, when present.
    pub const fn knot_interval(&self) -> Option<f64> {
        self.knot_interval
    }

    /// Sector coefficients at the two endpoints.
    pub const fn sector_coefficients(&self) -> [f64; 2] {
        self.sector_coefficients
    }
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
