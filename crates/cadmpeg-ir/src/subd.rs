// SPDX-License-Identifier: Apache-2.0
//! Subdivision-surface control cages.

mod admission;

use crate::features::FinitePoint3;
use crate::ids::SubdId;
use crate::math::{Point3, Vector3};
use crate::provenance::SourceObjectAssociation;
use crate::scalar::{FiniteReal, NonNegativeReal, PositiveReal};
use crate::units::UnitVector3;
use admission::{StandardAdmission, SubdAdmission};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A subdivision surface represented by its control cage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SubdSurface {
    /// Arena identity.
    pub id: SubdId,
    /// Subdivision scheme.
    pub scheme: SubdScheme,
    /// Control cage with admitted local topology and payloads.
    pub cage: SubdCage,
    /// Native source-object identity and effective display metadata.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_object"
    )]
    pub source_object: Option<SourceObjectAssociation>,
}

/// Admission error in a subdivision control cage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubdError {
    /// A cage value the carrier cannot admit. The carrier states this case;
    /// an outside caller states only [`Self::EditRefused`].
    #[non_exhaustive]
    Admission(String),
    /// An edit closure refused the value it was given, stating its own reason.
    EditRefused(String),
}

impl std::fmt::Display for SubdError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission(message) | Self::EditRefused(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SubdError {}

const EPS_SUBD_SYMMETRY_FRAME: f64 = 1.0e-9;

fn require_finite_point(field: &str, point: Point3) -> Result<FinitePoint3, SubdError> {
    FinitePoint3::new(point).ok_or_else(|| SubdError::Admission(format!("{field} must be finite")))
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
#[serde(deny_unknown_fields)]
struct SubdCageWire {
    /// Control vertices in cage order.
    vertices: Vec<SubdVertex>,
    /// Control edges in cage order.
    edges: Vec<SubdEdge>,
    /// Control faces in cage order.
    faces: Vec<SubdFace>,
    /// Editor symmetry blocks in cage order.
    #[serde(default)]
    symmetries: Vec<SubdSymmetry>,
}

impl TryFrom<SubdCageWire> for SubdCage {
    type Error = SubdError;

    fn try_from(wire: SubdCageWire) -> Result<Self, Self::Error> {
        let cage = Self {
            vertices: wire.vertices,
            edges: wire.edges,
            faces: wire.faces,
            symmetries: wire.symmetries,
        };
        cage.validate(&StandardAdmission)??;
        Ok(cage)
    }
}

impl SubdCage {
    /// Construct a cage with closed directed rings and valid local payloads.
    pub fn new(
        vertices: Vec<SubdVertex>,
        edges: Vec<SubdEdge>,
        faces: Vec<SubdFace>,
        symmetries: Vec<SubdSymmetry>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        let cage = Self {
            vertices,
            edges,
            faces,
            symmetries,
        };
        Ok(cage.validate(ctx)?.map(|()| cage))
    }

    /// Atomically edit admitted vertex copies while preserving cage invariants.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnOnce(&mut [SubdVertex]) -> Result<(), SubdError>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<(), SubdError>, cadmpeg_core::CodecError> {
        let mut storage = ctx.reserve_scoped(0, "SubD vertex edit storage")?;
        let mut vertices = storage.with_storage(|| {
            let copy_grips = |grips: &[Option<SubdSecondaryGrip>]| {
                ctx.try_collect_retained_with(grips, "copy SubD edit grips", |grip| {
                    Ok::<_, cadmpeg_core::CodecError>(grip.as_ref().map(|grip| SubdSecondaryGrip {
                        source_index: grip.source_index,
                        point: grip.point,
                        weight: grip.weight,
                    }))
                })
            };
            ctx.try_collect_retained_with(&self.vertices, "copy SubD edit vertices", |vertex| {
                let secondary_grips = match &vertex.secondary_grips {
                    None => None,
                    Some(layout) => Some(SubdVertexGripLayout {
                        direction: layout.direction,
                        wedges: ctx.try_collect_retained_with(
                            &layout.wedges,
                            "copy SubD edit wedges",
                            |wedge| {
                                Ok::<_, cadmpeg_core::CodecError>(match wedge {
                                    SubdGripWedge::Phantom {} => SubdGripWedge::Phantom {},
                                    SubdGripWedge::Slot {
                                        edge,
                                        sector_face,
                                        spokes,
                                        sectors,
                                    } => SubdGripWedge::Slot {
                                        edge: *edge,
                                        sector_face: *sector_face,
                                        spokes: copy_grips(spokes)?,
                                        sectors: copy_grips(sectors)?,
                                    },
                                })
                            },
                        )?,
                    }),
                };
                Ok::<_, cadmpeg_core::CodecError>(SubdVertex {
                    point: vertex.point,
                    tag: vertex.tag,
                    secondary_grips,
                })
            })
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(vertices.len()),
            "edit SubD vertices",
        )?;
        if let Err(error) = edit(&mut vertices) {
            return Ok(Err(error));
        }
        if let Err(error) = self.validate_vertices(&vertices, ctx)? {
            return Ok(Err(error));
        }
        self.vertices = storage.commit_value(vertices)?;
        Ok(Ok(()))
    }

    fn validate<A: SubdAdmission>(&self, admission: &A) -> Result<Result<(), SubdError>, A::Error> {
        for (index, edge) in self.edges.iter().enumerate() {
            admission.work(1, "validate SubD edge rows")?;
            for vertex in edge.vertices {
                if cadmpeg_core::decode::index_from_u32(vertex) >= self.vertices.len() {
                    return Ok(Err(admission.message(format_args!(
                        "edges[{index}].vertices contains an out-of-range index"
                    ))?));
                }
            }
        }
        for (index, face) in self.faces.iter().enumerate() {
            admission.work(1, "validate SubD face rows")?;
            let mut first = None;
            let mut previous = None;
            let mut closed = true;
            for use_ in &face.edges {
                admission.work(1, "validate SubD face edge references")?;
                let Some(edge) = self
                    .edges
                    .get(cadmpeg_core::decode::index_from_u32(use_.edge))
                else {
                    return Ok(Err(admission.message(format_args!(
                        "faces[{index}].edges references a missing edge"
                    ))?));
                };
                let endpoints = if use_.reversed {
                    [edge.vertices[1], edge.vertices[0]]
                } else {
                    edge.vertices
                };
                if closed {
                    if let Some(previous) = previous {
                        closed = previous == endpoints[0];
                    }
                }
                if first.is_none() {
                    first = Some(endpoints[0]);
                }
                previous = Some(endpoints[1]);
            }
            if closed {
                if let (Some(first), Some(previous)) = (first, previous) {
                    closed = previous == first;
                }
            }
            if !closed {
                return Ok(Err(admission.message(format_args!(
                    "faces[{index}].edges is not a directed closed ring"
                ))?));
            }
        }
        if let Err(error) = self.validate_vertices(&self.vertices, admission)? {
            return Ok(Err(error));
        }
        for symmetry in &self.symmetries {
            admission.work(1, "validate SubD symmetry rows")?;
            for (field, pairs, count) in [
                ("face_pairs", &symmetry.face_pairs, self.faces.len()),
                ("edge_pairs", &symmetry.edge_pairs, self.edges.len()),
                ("vertex_pairs", &symmetry.vertex_pairs, self.vertices.len()),
            ] {
                for pair in pairs {
                    admission.work(1, "validate SubD symmetry indices")?;
                    for index in pair {
                        if cadmpeg_core::decode::index_from_u32(*index) >= count {
                            return Ok(Err(admission.message(format_args!(
                                "symmetries.{field} contains an out-of-range index"
                            ))?));
                        }
                    }
                }
            }
        }
        Ok(Ok(()))
    }

    fn validate_vertices<A: SubdAdmission>(
        &self,
        vertices: &[SubdVertex],
        admission: &A,
    ) -> Result<Result<(), SubdError>, A::Error> {
        let mut storage = admission.storage()?;
        let mut grip_indices = std::collections::BTreeSet::new();
        for (index, vertex) in vertices.iter().enumerate() {
            admission.work(1, "validate SubD vertex rows")?;
            let Some(layout) = &vertex.secondary_grips else {
                continue;
            };
            for wedge in &layout.wedges {
                admission.work(1, "validate SubD grip wedges")?;
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
                    let Some(edge) = self.edges.get(cadmpeg_core::decode::index_from_u32(*edge))
                    else {
                        return Ok(Err(admission.message(format_args!(
                            "vertices[{index}].secondary_grips edge is out of range"
                        ))?));
                    };
                    let mut incident = false;
                    for owner in edge.vertices {
                        if cadmpeg_core::decode::index_from_u32(owner) == index {
                            incident = true;
                            break;
                        }
                    }
                    if !incident {
                        return Ok(Err(admission.message(format_args!(
                            "vertices[{index}].secondary_grips edge is not incident to its owner"
                        ))?));
                    }
                }
                if let Some(face) = sector_face {
                    let Some(face) = self.faces.get(cadmpeg_core::decode::index_from_u32(*face))
                    else {
                        return Ok(Err(admission.message(format_args!(
                            "vertices[{index}].secondary_grips sector_face is out of range"
                        ))?));
                    };
                    let mut incident = false;
                    for use_ in &face.edges {
                        admission.work(1, "validate SubD grip face edges")?;
                        for owner in
                            self.edges[cadmpeg_core::decode::index_from_u32(use_.edge)].vertices
                        {
                            if cadmpeg_core::decode::index_from_u32(owner) == index {
                                incident = true;
                                break;
                            }
                        }
                        if incident {
                            break;
                        }
                    }
                    if !incident {
                        return Ok(Err(admission.message(format_args!("vertices[{index}].secondary_grips sector_face is not incident to its owner"))?));
                    }
                }
                for grip in spokes.iter().chain(sectors) {
                    admission.work(1, "validate SubD grip slots")?;
                    let Some(grip) = grip else {
                        continue;
                    };
                    if !admission.insert(&mut storage, &mut grip_indices, grip.source_index)? {
                        return Ok(Err(admission.message(format_args!(
                            "vertices[{index}].secondary_grips repeats a source_index"
                        ))?));
                    }
                }
            }
        }
        Ok(Ok(()))
    }
}

/// A symmetry plane frame carried by a T-spline editor block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdPlaneFrameWire")]
pub struct SubdPlaneFrame {
    /// A point on the plane in document length units.
    origin: FinitePoint3,
    /// First unit in-plane axis.
    first_axis: UnitVector3,
    /// Second unit in-plane axis.
    second_axis: UnitVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubdPlaneFrameWire {
    /// A point on the plane in document length units.
    origin: Point3,
    /// First unit in-plane axis.
    first_axis: Vector3,
    /// Second unit in-plane axis.
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
        let origin = require_finite_point("origin", origin)?;
        let first_axis = UnitVector3::new(first_axis).ok_or_else(|| {
            SubdError::Admission("first_axis must be finite and unit length".into())
        })?;
        let second_axis = UnitVector3::new(second_axis).ok_or_else(|| {
            SubdError::Admission("second_axis must be finite and unit length".into())
        })?;
        if first_axis.as_raw().dot(*second_axis.as_raw()).abs() > EPS_SUBD_SYMMETRY_FRAME {
            return Err(SubdError::Admission(
                "first_axis and second_axis must be orthogonal".into(),
            ));
        }
        Ok(Self {
            origin,
            first_axis,
            second_axis,
        })
    }
}

/// Kind-specific controls for a T-spline symmetry block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubdSymmetryKind {
    /// One-to-one correspondence across the symmetry plane.
    Correspondence {},
    /// Radial editor symmetry with native segment and sweep controls.
    Radial(SubdRadialSymmetry),
}

/// Admitted radial controls and selector-preserving maps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdRadialSymmetryWire")]
pub struct SubdRadialSymmetry {
    segments: std::num::NonZeroU32,
    sweep: crate::scalar::FiniteReal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    radial_maps: Vec<SubdRadialSymmetryMap>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubdRadialSymmetryWire {
    /// Number of radial segments.
    segments: std::num::NonZeroU32,
    /// Finite native radial sweep.
    sweep: f64,
    /// Native maps with distinct selectors and distinct sources within each map.
    #[serde(default)]
    radial_maps: Vec<SubdRadialSymmetryMap>,
}

impl TryFrom<SubdRadialSymmetryWire> for SubdRadialSymmetry {
    type Error = SubdError;

    fn try_from(wire: SubdRadialSymmetryWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.segments,
            wire.sweep,
            wire.radial_maps,
            &StandardAdmission,
        )?
    }
}

impl SubdRadialSymmetry {
    fn new<A: SubdAdmission>(
        segments: std::num::NonZeroU32,
        sweep: f64,
        radial_maps: Vec<SubdRadialSymmetryMap>,
        admission: &A,
    ) -> Result<Result<Self, SubdError>, A::Error> {
        let Some(sweep) = FiniteReal::new(sweep) else {
            return Ok(Err(
                admission.message(format_args!("kind.radial.sweep must be finite"))?
            ));
        };
        Self::from_parts(segments, sweep, radial_maps, admission)
    }

    fn from_parts<A: SubdAdmission>(
        segments: std::num::NonZeroU32,
        sweep: FiniteReal,
        radial_maps: Vec<SubdRadialSymmetryMap>,
        admission: &A,
    ) -> Result<Result<Self, SubdError>, A::Error> {
        let mut selector_storage = admission.storage()?;
        let mut selectors = std::collections::BTreeSet::new();
        for map in &radial_maps {
            admission.work(1, "validate SubD radial maps")?;
            if !admission.insert(&mut selector_storage, &mut selectors, map.selector)? {
                return Ok(Err(
                    admission.message(format_args!("radial_maps repeats a selector"))?
                ));
            }
            let mut source_storage = admission.storage()?;
            let mut sources = std::collections::BTreeSet::new();
            for [source, _] in &map.pairs {
                admission.work(1, "validate SubD radial map sources")?;
                if !admission.insert(&mut source_storage, &mut sources, *source)? {
                    return Ok(Err(
                        admission.message(format_args!("radial_maps.pairs repeats a source"))?
                    ));
                }
            }
        }
        Ok(Ok(Self {
            segments,
            sweep,
            radial_maps,
        }))
    }
}

impl SubdSymmetryKind {
    /// Admit radial controls with finite sweep and distinct map selectors and sources.
    pub fn radial(
        segments: std::num::NonZeroU32,
        sweep: f64,
        radial_maps: Vec<SubdRadialSymmetryMap>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        Ok(SubdRadialSymmetry::new(segments, sweep, radial_maps, ctx)?.map(Self::Radial))
    }

    /// Admit radial maps with a sweep that the reader already checked.
    pub fn radial_from_parts(
        segments: std::num::NonZeroU32,
        sweep: FiniteReal,
        radial_maps: Vec<SubdRadialSymmetryMap>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        Ok(SubdRadialSymmetry::from_parts(segments, sweep, radial_maps, ctx)?.map(Self::Radial))
    }
}

/// Selector of one native radial-symmetry map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

impl cadmpeg_core::decode::cost::DecodeCost for SubdRadialMapSelector {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));

    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

/// One selector-preserving native radial-symmetry map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SubdRadialSymmetryMap {
    /// Native map selector. Its element namespace is format-native.
    pub selector: SubdRadialMapSelector,
    /// Native source/target identifier pairs.
    pub pairs: Vec<[u64; 2]>,
}

/// Typed editor symmetry state for one subdivision cage.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdSymmetryWire", into = "SubdSymmetryWire")]
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

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "SubdSymmetry"))]
#[serde(deny_unknown_fields)]
struct SubdSymmetryWire {
    /// Symmetry mode and its radial controls, when present.
    kind: SubdSymmetryKind,
    /// Geometric symmetry-plane frame.
    plane: SubdPlaneFrame,
    /// Forward face correspondences for a topology-addressed symmetry block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    face_pairs: Vec<[u32; 2]>,
    /// Forward edge correspondences for a topology-addressed symmetry block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edge_pairs: Vec<[u32; 2]>,
    /// Forward vertex correspondences for a topology-addressed symmetry block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_pairs: Vec<[u32; 2]>,
}

impl SubdSymmetry {
    /// Construct symmetry state with admitted distinct correspondences.
    pub fn new(
        kind: SubdSymmetryKind,
        plane: SubdPlaneFrame,
        face_pairs: Vec<[u32; 2]>,
        edge_pairs: Vec<[u32; 2]>,
        vertex_pairs: Vec<[u32; 2]>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        Self::from_rows(kind, plane, face_pairs, edge_pairs, vertex_pairs, ctx)
    }

    fn from_rows<A: SubdAdmission>(
        kind: SubdSymmetryKind,
        plane: SubdPlaneFrame,
        face_pairs: Vec<[u32; 2]>,
        edge_pairs: Vec<[u32; 2]>,
        vertex_pairs: Vec<[u32; 2]>,
        admission: &A,
    ) -> Result<Result<Self, SubdError>, A::Error> {
        for (field, pairs) in [
            ("face_pairs", &face_pairs),
            ("edge_pairs", &edge_pairs),
            ("vertex_pairs", &vertex_pairs),
        ] {
            let mut storage = admission.storage()?;
            let mut sources = std::collections::BTreeSet::new();
            let mut targets = std::collections::BTreeSet::new();
            for [source, target] in pairs {
                admission.work(1, "validate SubD correspondence pairs")?;
                if !admission.insert(&mut storage, &mut sources, *source)?
                    || !admission.insert(&mut storage, &mut targets, *target)?
                {
                    return Ok(Err(
                        admission.message(format_args!("{field} repeats a source or target"))?
                    ));
                }
            }
        }
        Ok(Ok(Self {
            kind,
            plane,
            face_pairs,
            edge_pairs,
            vertex_pairs,
        }))
    }
}

impl TryFrom<SubdSymmetryWire> for SubdSymmetry {
    type Error = SubdError;

    fn try_from(wire: SubdSymmetryWire) -> Result<Self, Self::Error> {
        Self::from_rows(
            wire.kind,
            wire.plane,
            wire.face_pairs,
            wire.edge_pairs,
            wire.vertex_pairs,
            &StandardAdmission,
        )?
    }
}

/// Subdivision scheme used by a control cage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
    point: FinitePoint3,
    /// Subdivision vertex tag.
    pub tag: SubdVertexTag,
    /// Optional secondary-grip topology owned by this vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_grips"
    )]
    secondary_grips: Option<SubdVertexGripLayout>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubdVertexWire {
    /// Vertex position.
    point: Point3,
    /// Subdivision vertex tag.
    tag: SubdVertexTag,
    /// Optional secondary-grip topology owned by this vertex.
    #[serde(default, deserialize_with = "deserialize_secondary_grips")]
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
        let point = require_finite_point("point", point)?;
        Ok(Self::from_parts(point, tag, secondary_grips))
    }

    /// Construct a control vertex from its admitted finite position.
    #[must_use]
    pub fn from_parts(
        point: FinitePoint3,
        tag: SubdVertexTag,
        secondary_grips: Option<SubdVertexGripLayout>,
    ) -> Self {
        Self {
            point,
            tag,
            secondary_grips,
        }
    }

    /// Vertex position in document units.
    pub const fn point(&self) -> FinitePoint3 {
        self.point
    }

    /// Replace the vertex position.
    pub fn set_point(&mut self, point: FinitePoint3) {
        self.point = point;
    }
}

/// Compass direction of the root edge in a control-cage grid frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct SubdVertexGripLayoutWire {
    /// Direction of the native root edge; wedge zero is the north slot.
    direction: SubdGripDirection,
    /// Wedges in north-anchored order.
    wedges: Vec<SubdGripWedge>,
}

impl TryFrom<SubdVertexGripLayoutWire> for SubdVertexGripLayout {
    type Error = SubdError;

    fn try_from(wire: SubdVertexGripLayoutWire) -> Result<Self, Self::Error> {
        Self::from_wedges(wire.direction, wire.wedges, &StandardAdmission)?
    }
}

impl SubdVertexGripLayout {
    /// Admit a nonempty cyclic fan with sector arity equal to adjacent spoke products.
    pub fn new(
        direction: SubdGripDirection,
        wedges: Vec<SubdGripWedge>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        Self::from_wedges(direction, wedges, ctx)
    }

    fn from_wedges<A: SubdAdmission>(
        direction: SubdGripDirection,
        wedges: Vec<SubdGripWedge>,
        admission: &A,
    ) -> Result<Result<Self, SubdError>, A::Error> {
        if wedges.is_empty() {
            return Ok(Err(
                admission.message(format_args!("secondary_grips.wedges is empty"))?
            ));
        }
        let spoke_count = |wedge: &SubdGripWedge| match wedge {
            SubdGripWedge::Phantom {} => 0,
            SubdGripWedge::Slot { spokes, .. } => spokes.len(),
        };
        for (index, (wedge, next)) in wedges.iter().zip(wedges.iter().cycle().skip(1)).enumerate() {
            admission.work(1, "validate SubD wedge arity")?;
            let sector_count = match wedge {
                SubdGripWedge::Phantom {} => 0,
                SubdGripWedge::Slot { sectors, .. } => sectors.len(),
            };
            if spoke_count(wedge).checked_mul(spoke_count(next)) != Some(sector_count) {
                return Ok(Err(admission.message(format_args!(
                    "secondary_grips.wedges[{index}].sectors has invalid arity"
                ))?));
            }
        }
        Ok(Ok(Self { direction, wedges }))
    }
}

/// One wedge in a secondary-grip layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubdGripWedge {
    /// Boundary padding with no topology or grip data.
    Phantom {},
    /// One topology and grip slot in the vertex fan.
    Slot {
        /// IR edge for this fan slot.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        edge: Option<u32>,
        /// Face in the sector following this slot, or `None` for a boundary gap.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        sector_face: Option<u32>,
        /// Spoke grips ordered nearest-first from the owning vertex.
        spokes: Vec<Option<SubdSecondaryGrip>>,
        /// Sector-grid grips ordered by the spoke-k position, then the
        /// next-spoke position, with `S[k] * S[k + 1]` slots.
        sectors: Vec<Option<SubdSecondaryGrip>>,
    },
}

/// A secondary grip point and its source grip-array identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SubdSecondaryGripWire")]
pub struct SubdSecondaryGrip {
    /// Index in the source cage's `0g` grip array.
    source_index: u32,
    /// Grip position in document units.
    point: FinitePoint3,
    /// Positive rational grip weight.
    weight: PositiveReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubdSecondaryGripWire {
    /// Index in the source cage's `0g` grip array.
    source_index: u32,
    /// Grip position in document units.
    point: Point3,
    /// Positive rational grip weight.
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
        let point = require_finite_point("point", point)?;
        let weight = PositiveReal::new(weight)
            .ok_or_else(|| SubdError::Admission("weight must be finite and positive".into()))?;
        Ok(Self {
            source_index,
            point,
            weight,
        })
    }

    /// Construct a grip with a weight that the reader already checked.
    pub fn from_parts(
        source_index: u32,
        point: Point3,
        weight: PositiveReal,
    ) -> Result<Self, SubdError> {
        let point = require_finite_point("point", point)?;
        Ok(Self {
            source_index,
            point,
            weight,
        })
    }
}

/// A control-cage vertex tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
    sharpness: [NonNegativeReal; 2],
    /// Subdivision edge tag.
    pub tag: SubdEdgeTag,
    /// Parametric knot interval, when the source cage exposes one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_knot_interval"
    )]
    knot_interval: Option<PositiveReal>,
    /// Sector coefficients at the two endpoints.
    sector_coefficients: [FiniteReal; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SubdEdgeWire {
    /// Indices of the two distinct endpoint vertices.
    vertices: [u32; 2],
    /// Sharpness at the start and end endpoints.
    sharpness: [f64; 2],
    /// Subdivision edge tag.
    tag: SubdEdgeTag,
    /// Parametric knot interval, when the source cage exposes one.
    #[serde(default, deserialize_with = "deserialize_knot_interval")]
    knot_interval: Option<f64>,
    /// Sector coefficients at the two endpoints.
    sector_coefficients: [f64; 2],
}

impl TryFrom<SubdEdgeWire> for SubdEdge {
    type Error = SubdError;

    fn try_from(wire: SubdEdgeWire) -> Result<Self, Self::Error> {
        Self::admit_raw_controls(
            wire.vertices,
            wire.sharpness,
            wire.tag,
            || match wire.knot_interval {
                None => Ok(Ok(None)),
                Some(value) => Ok(PositiveReal::new(value).map(Some).ok_or_else(|| {
                    SubdError::Admission("knot_interval must be finite and positive".into())
                })),
            },
            wire.sector_coefficients,
            |message| Ok(SubdError::Admission(message.into())),
        )?
    }
}

impl SubdEdge {
    /// Construct an edge from admitted numeric controls. Only distinct
    /// endpoints remain to be checked.
    pub fn from_controls(
        vertices: [u32; 2],
        sharpness: [NonNegativeReal; 2],
        tag: SubdEdgeTag,
        knot_interval: Option<PositiveReal>,
        sector_coefficients: [FiniteReal; 2],
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        ctx.charge_work(0, "SubD edge endpoints")?;
        if vertices[0] == vertices[1] {
            return Ok(Err(SubdError::Admission(ctx.copy_retained_text(
                "vertices must name distinct endpoints",
                "SubD edge admission error",
            )?)));
        }
        Ok(Ok(Self {
            vertices,
            sharpness,
            tag,
            knot_interval,
            sector_coefficients,
        }))
    }

    /// Admit raw numeric controls under the caller decode policy.
    pub fn new(
        vertices: [u32; 2],
        sharpness: [f64; 2],
        tag: SubdEdgeTag,
        knot_interval: Option<f64>,
        sector_coefficients: [f64; 2],
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        ctx.charge_work(0, "SubD edge controls")?;
        Self::admit_raw_controls(
            vertices,
            sharpness,
            tag,
            || match knot_interval {
                None => Ok(Ok(None)),
                Some(value) => match PositiveReal::new(value) {
                    Some(value) => Ok(Ok(Some(value))),
                    None => Ok(Err(SubdError::Admission(ctx.copy_retained_text(
                        "knot_interval must be finite and positive",
                        "SubD edge admission error",
                    )?))),
                },
            },
            sector_coefficients,
            |message| {
                Ok(SubdError::Admission(ctx.copy_retained_text(
                    message,
                    "SubD edge admission error",
                )?))
            },
        )
    }

    /// Admit raw endpoint controls and an already positive interval.
    pub fn from_parts(
        vertices: [u32; 2],
        sharpness: [f64; 2],
        tag: SubdEdgeTag,
        knot_interval: Option<PositiveReal>,
        sector_coefficients: [f64; 2],
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, SubdError>, cadmpeg_core::CodecError> {
        ctx.charge_work(0, "SubD edge controls")?;
        Self::admit_raw_controls(
            vertices,
            sharpness,
            tag,
            || Ok(Ok(knot_interval)),
            sector_coefficients,
            |message| {
                Ok(SubdError::Admission(ctx.copy_retained_text(
                    message,
                    "SubD edge admission error",
                )?))
            },
        )
    }

    fn admit_raw_controls<E>(
        vertices: [u32; 2],
        sharpness: [f64; 2],
        tag: SubdEdgeTag,
        interval: impl FnOnce() -> Result<Result<Option<PositiveReal>, SubdError>, E>,
        sector_coefficients: [f64; 2],
        mut error: impl FnMut(&'static str) -> Result<SubdError, E>,
    ) -> Result<Result<Self, SubdError>, E> {
        if vertices[0] == vertices[1] {
            return Ok(Err(error("vertices must name distinct endpoints")?));
        }
        let [start, end] = sharpness.map(NonNegativeReal::new);
        let (Some(start), Some(end)) = (start, end) else {
            return Ok(Err(error("sharpness must be finite and non-negative")?));
        };
        let knot_interval = match interval()? {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        let [first, second] = sector_coefficients.map(FiniteReal::new);
        let (Some(first), Some(second)) = (first, second) else {
            return Ok(Err(error("sector_coefficients must be finite")?));
        };
        Ok(Ok(Self {
            vertices,
            sharpness: [start, end],
            tag,
            knot_interval,
            sector_coefficients: [first, second],
        }))
    }
}

/// A control-cage edge tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct SubdFaceWire {
    /// Directed edge uses in boundary order.
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
            return Err(SubdError::Admission(
                "edges must contain at least three uses".into(),
            ));
        }
        Ok(Self { edges })
    }
}

/// One directed use of a subdivision edge in a face ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SubdEdgeUse {
    /// Index into the parent surface's edge array.
    pub edge: u32,
    /// Whether this use traverses the edge from its second endpoint.
    pub reversed: bool,
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_source_object,
    SourceObjectAssociation,
    "source_object"
);
cadmpeg_core::named_optional_field!(
    deserialize_secondary_grips,
    SubdVertexGripLayout,
    "secondary_grips"
);
cadmpeg_core::named_optional_field!(deserialize_knot_interval, f64, "knot_interval");

mod identity_rewrite;

mod serialization;
