// SPDX-License-Identifier: Apache-2.0
//! Format-independent native record types retained from a decoded ASM stream.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, ShellId, SurfaceId, VertexId};

/// Kernel continuity classification stored on one solved ASM edge record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EdgeContinuity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep edge carrying the classification.
    pub edge: EdgeId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Native curve-parameterization sense before IR carrier normalization.
    pub sense: cadmpeg_ir::topology::Sense,
    /// Native continuity token, normally `tangent` or `unknown`.
    pub continuity: String,
}

/// Native owner-coedge selector stored on one ASM edge record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EdgeOwnership {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep edge carrying the selector.
    pub edge: EdgeId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Selected coedge, or null when the native edge has no owner back-reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_coedge: Option<CoedgeId>,
}

/// Native owner-edge and endpoint-slot fields stored on one ASM vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VertexOwnership {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep vertex carrying the fields.
    pub vertex: VertexId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Edge selected as this vertex record's native owner.
    pub owning_edge: EdgeId,
    /// Endpoint slot on `owning_edge`: `0` for start, `1` for end.
    pub endpoint_index: u8,
}

/// Conditional containment direction on a double-sided ASM face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FaceContainment {
    /// The face bounds the inside side of its surface.
    In,
    /// The face bounds the outside side of its surface.
    Out,
}

/// Native sidedness fields stored on one ASM face record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FaceSidedness {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep face carrying the fields.
    pub face: FaceId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Sense token stored in the native face record before carrier normalization.
    pub native_sense: cadmpeg_ir::topology::Sense,
    /// IR sense produced when `native_sense` was decoded.
    pub normalized_sense: cadmpeg_ir::topology::Sense,
    /// Conditional containment direction; absence denotes a single-sided face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment: Option<FaceContainment>,
}

/// Native leading tolerance slots retained from one tolerant ASM vertex
/// record. The record's three f64 tolerance slots are three independent
/// tolerance evaluations, each using `-1` as its unset sentinel; the third
/// slot is the effective vertex tolerance and is stored on the vertex, while
/// the first two are retained here verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TolerantVertexTail {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep vertex carrying the tolerant record.
    pub vertex: VertexId,
    /// Source SAB record index.
    pub record_index: u32,
    /// The first two independent tolerance evaluations, retained verbatim in
    /// native centimetres; `-1` denotes an unset evaluation.
    pub leading_tolerances: [f64; 2],
    /// Version-gated trailing LONG following the evaluated tolerance,
    /// retained verbatim; absent in older streams, a small non-negative
    /// per-entity change counter when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailing_field: Option<i64>,
    /// Whether the evaluated tolerance slot holds the `-1` unset sentinel.
    /// The sentinel is a marker rather than a length, so the neutral vertex
    /// carries no tolerance and this record keeps the fact.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub evaluated_unset: bool,
}

/// Native tail retained from one tolerant ASM edge record: the entity
/// serializer revision stamp followed by a version-gated LONG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TolerantEdgeTail {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep edge carrying the tolerant record.
    pub edge: EdgeId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Per-entity serializer revision stamp following the model-space
    /// tolerance, matching the stream's revision value space.
    pub entity_revision: i64,
    /// Version-gated trailing LONG following the revision stamp, retained
    /// verbatim; absent in older streams, a small non-negative per-entity
    /// change counter when present.
    pub trailing_field: Option<i64>,
}

/// Parameter interval stored by one tolerant ASM coedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TolerantCoedgeParameters {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep coedge carrying the tolerant interval.
    pub coedge: CoedgeId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Native start and end parameters following the base coedge fields.
    pub parameter_range: [f64; 2],
    /// Release-selected fixed fields following the parameter interval.
    #[serde(default)]
    pub extension: TolerantCoedgeExtension,
}

/// Release-selected fixed fields following a tolerant-coedge parameter interval.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "layout")]
pub enum TolerantCoedgeExtension {
    /// Releases below 215 have no fixed extension fields.
    #[default]
    None,
    /// Releases 215 through 219 carry one nullable entity reference.
    Reference {
        /// Referenced record index; `None` is the native null reference.
        target: Option<i64>,
    },
    /// Modern releases carry no embedded tolerant-curve payload.
    Empty {
        /// Nullable record reference preceding the zero selector.
        target: Option<i64>,
    },
    /// Modern releases carry one balanced embedded tolerant-curve payload.
    EmbeddedCurve {
        /// Nullable record reference preceding the one selector.
        target: Option<i64>,
        /// Whether the embedded intcurve is evaluated with parameter negation.
        #[serde(alias = "flag")]
        curve_reversed: bool,
        /// Number of tokens inside the balanced outer subtype delimiters.
        payload_token_count: u32,
        /// Optional parameter interval following the embedded subtype.
        parameter_range: Option<[f64; 2]>,
    },
}

/// Zero-payload ASM surface sentinel whose shape is supplied only by tessellation attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MeshSurfaceSentinel {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Unknown exact-surface placeholder emitted for the sentinel record.
    pub surface: SurfaceId,
    /// Source SAB record index.
    pub record_index: u32,
}

/// Native side classification stored on an ASM wire record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireSide {
    /// Wire bounds the inside side.
    In,
    /// Wire bounds the outside side.
    Out,
}

/// Native wire record projected onto one neutral-IR shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WireTopology {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Neutral shell containing the wire.
    pub shell: ShellId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Ordered edge ring owned through the wire's first-coedge reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeId>,
    /// Isolated vertex owned when the first-coedge reference is null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_vertex: Option<VertexId>,
    /// Native side classification.
    pub side: WireSide,
}

/// Native Design-join key stored on one ASM body record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BodyNativeKey {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved body carrying the key.
    pub body: BodyId,
    /// Source SAB body record index.
    pub record_index: u32,
    /// Zero-based body-record position within the BREP blob.
    #[serde(default)]
    pub body_ordinal: u32,
    /// Basename of the BREP blob containing this body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_brep: Option<String>,
    /// Non-negative Design-join key; absence is the native `-1` null value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asm_body_key: Option<u64>,
}

/// Native rotation, reflection, and shear classifications on an ASM transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TransformHints {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved body referencing the transform record.
    pub body: BodyId,
    /// Source SAB transform record index.
    pub record_index: u32,
    /// The linear transform includes rotation.
    pub rotation: bool,
    /// The linear transform includes reflection.
    pub reflection: bool,
    /// The linear transform includes shear.
    pub shear: bool,
}
