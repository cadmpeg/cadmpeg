// SPDX-License-Identifier: Apache-2.0
//! Fusion parametric-design records and links to the solved B-rep.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, ShellId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

/// Provenance link from a solved B-rep coedge to its source sketch curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchCurveLink {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep coedge this link provenances back to a sketch curve.
    pub coedge: CoedgeId,
    /// Numeric design-entity id of the source sketch-curve record.
    pub sketch_curve_id: i64,
    /// Signed variant of `sketch_curve_id` carrying orientation of the sketch curve
    /// relative to the coedge, when the source record encoded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_reference: Option<i64>,
    /// Source role tag distinguishing how the sketch curve participates in the link
    /// (e.g. profile edge vs. construction reference).
    pub role: i64,
    /// Source closure/continuity tag of the sketch curve at this link.
    pub closure: i64,
}

/// Persistent Fusion design identifier attached to a solved B-rep entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentDesignLink {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep entity this persistent Fusion design id is attached to.
    pub target: AttributeTarget,
    /// Fusion persistent design-entity id string, stable across regeneration.
    pub design_id: String,
    /// Native entity-class discriminator: body `3`, face `2`, or edge `1`.
    pub entity_kind: i64,
    /// Design-stream reference paired with this persistent identifier.
    pub design_reference: i64,
    /// Position of this id in the entity's persistent-id history, in assignment order.
    pub ordinal: u32,
    /// Whether this is the active persistent id for `target`, as opposed to a
    /// superseded historical id retained for provenance.
    pub is_current: bool,
}

/// Native face/edge tag group linking a solved subentity to design records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentSubentityTag {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep face or edge carrying this tag group.
    pub target: AttributeTarget,
    /// Native selector stored before the tag token.
    pub selector: i64,
    /// Native UTF-8 tag token. Numeric strings and `-1` retain their spelling.
    pub token: String,
    /// Ordered signed Design-stream references carried by this group.
    pub design_references: Vec<i64>,
    /// Position of this group in the owning attribute record.
    pub ordinal: u32,
}

/// Original authoring time attached to a solved ASM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreationTimestamp {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep entity carrying the timestamp attribute.
    pub target: AttributeTarget,
    /// Source SAB record index of the timestamp attribute.
    pub record_index: u32,
    /// Creation time as microseconds since the Unix epoch.
    pub unix_microseconds: f64,
}

/// Kernel continuity classification stored on one solved ASM edge record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FaceContainment {
    /// The face bounds the inside side of its surface.
    In,
    /// The face bounds the outside side of its surface.
    Out,
}

/// Native sidedness fields stored on one ASM face record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// Native f32 tail retained from one tolerant ASM vertex record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TolerantVertexTail {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep vertex carrying the tolerant record.
    pub vertex: VertexId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Two trailing f32 slots following the model-space tolerance.
    pub trailing_floats: [f32; 2],
}

/// Native integer tail retained from one tolerant ASM edge record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TolerantEdgeTail {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep edge carrying the tolerant record.
    pub edge: EdgeId,
    /// Source SAB record index.
    pub record_index: u32,
    /// Two trailing LONG slots following the model-space tolerance.
    pub trailing_integers: [i64; 2],
}

/// Parameter interval stored by one tolerant ASM coedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
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
        /// Boolean stored immediately before the embedded subtype.
        flag: bool,
        /// Number of tokens inside the balanced outer subtype delimiters.
        payload_token_count: u32,
        /// Optional parameter interval following the embedded subtype.
        parameter_range: Option<[f64; 2]>,
    },
}

/// Zero-payload ASM surface sentinel whose shape is supplied only by tessellation attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshSurfaceSentinel {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Unknown exact-surface placeholder emitted for the sentinel record.
    pub surface: cadmpeg_ir::ids::SurfaceId,
    /// Source SAB record index.
    pub record_index: u32,
}

/// Native side classification stored on an ASM wire record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireSide {
    /// Wire bounds the inside side.
    In,
    /// Wire bounds the outside side.
    Out,
}

/// Native wire record projected onto one neutral-IR shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// Design `BulkStream` regeneration-recipe family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionRecipeKind {
    /// Recipe regenerates a whole body.
    Body,
    /// Recipe regenerates a single face.
    Face,
    /// Recipe regenerates a face bounded by an explicit region.
    BoundedFace,
    /// Recipe regenerates a single edge.
    Edge,
    /// Recipe regenerates a single vertex.
    Vertex,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConstructionRecipe {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this recipe's family marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of `record_index` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_index_offset: Option<u64>,
    /// Topology kind this recipe regenerates on replay.
    pub kind: ConstructionRecipeKind,
    /// Design entity id of the body this recipe is keyed to, if the source record
    /// carried a `generic_tag_attrib_def` construction id; `None` for body-less recipes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id: Option<String>,
    /// Byte offset of `design_id` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id_offset: Option<u64>,
    /// Whether `design_id` is stored as a binary little-endian u32 rather than ASCII.
    #[serde(default)]
    pub design_id_binary_u32: bool,
    /// Position of this recipe in the `BulkStream` recipe sequence, in source order.
    pub recipe_index: u32,
    /// Source `BulkStream` record index this recipe was decoded from.
    pub record_index: i32,
}

/// Semantic family of one Design parameter record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignParameterKind {
    /// A document-level named user parameter.
    User,
    /// A dimensional constraint parameter.
    Dimension,
    /// A parameter consumed by a construction feature.
    Feature,
}

/// One indexed Design parameter or expression record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignParameter {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Parameter-family discriminator: `6` for `TangencyWeight`, otherwise `0`.
    pub prefix_value: u64,
    /// Byte offset of `prefix_value`.
    pub prefix_value_offset: u64,
    /// Source ordering value stored by the parameter record.
    pub source_ordinal: u32,
    /// Indexed owner record for feature and dimension parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_record_index: Option<u32>,
    /// Literal or symbolic source expression.
    pub expression: String,
    /// Byte offset of the expression's UTF-16LE code units.
    pub expression_offset: u64,
    /// Source family label such as `User Parameter`, `AlongDistance`, or
    /// `Linear Dimension-2`.
    pub source_kind: String,
    /// Byte offset of the source-family UTF-16LE code units.
    pub source_kind_offset: u64,
    /// Parameter family derived from `source_kind`.
    pub kind: DesignParameterKind,
    /// Declared unit token; absent for dimensionless and Boolean parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Byte offset of the unit's UTF-16LE code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_offset: Option<u64>,
    /// Source parameter name or dimension identifier.
    pub name: String,
    /// Byte offset of the name's UTF-16LE code units.
    pub name_offset: u64,
    /// Evaluated scalar in the record's native unit convention.
    pub evaluated_value: f64,
    /// Byte offset of `evaluated_value`.
    pub evaluated_value_offset: u64,
}

/// Fixed-width indexed record that owns one Design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignParameterOwner {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Feature or sketch record that scopes this parameter.
    pub scope_record_index: u32,
    /// Position among parameters in the same scope.
    pub local_ordinal: u32,
    /// Evaluated scalar duplicated from the parameter record.
    pub evaluated_value: f64,
    /// Byte offset of `evaluated_value`.
    pub evaluated_value_offset: u64,
    /// Indexed parameter record owned by this frame.
    pub parameter_record_index: u32,
    /// Position among all feature- and dimension-owned parameters.
    pub owned_ordinal: u32,
    /// Source owner-frame variant flag.
    pub variant: u8,
    /// Paired indexed record following the parameter record.
    pub companion_record_index: u32,
}

/// Fixed prefix of the indexed record paired with a Design parameter owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignParameterCompanion {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the indexed record header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Indexed parameter-owner record referenced by this prefix.
    pub owner_record_index: u32,
    /// Nonzero Unix-epoch timestamp in microseconds.
    #[serde(alias = "opaque_value")]
    pub timestamp_micros: u64,
    /// Byte offset of `timestamp_micros`.
    #[serde(alias = "opaque_value_offset")]
    pub timestamp_micros_offset: u64,
    /// First byte owned after the fixed companion prefix.
    #[serde(default)]
    pub payload_byte_offset: u64,
    /// Number of bytes owned before the next sibling Design record.
    #[serde(default)]
    pub payload_byte_length: u64,
    /// Construction recipes contained by the owned payload, in byte order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owned_recipe_ids: Vec<String>,
}

/// Indexed record that directly contains one construction recipe owned by a
/// dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesignDimensionRecipeRecord {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this indexed record.
    pub companion_record_index: u32,
    /// Zero-based recipe position within the companion payload.
    pub recipe_ordinal: u32,
    /// Construction recipe contained by this indexed record.
    pub recipe_id: String,
    /// Byte offset of the indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Number of bytes from this header to the next indexed header or the end
    /// of the companion-owned payload.
    pub frame_length: u64,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference tails decoded from the prefix.
    pub references: Vec<DesignDimensionRecipeReference>,
    /// Byte offset of the first i32 after the recipe-family name.
    pub program_offset: u64,
    /// Complete little-endian i32 program through the indexed-record boundary.
    pub program: Vec<i32>,
    /// Edge operands whose complete post-prologue recipe program occurs as a
    /// contiguous subsequence of this program.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matching_edge_operand_ids: Vec<String>,
}

/// One persistent Design selector/reference tail in a dimension recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesignDimensionRecipeReference {
    /// ASCII persistent-subentity selector token.
    pub token: String,
    /// Byte offset of the token bytes.
    pub token_offset: u64,
    /// Persistent Design reference paired with `token`.
    pub design_reference: i64,
    /// Byte offset of `design_reference`.
    pub design_reference_offset: u64,
    /// Active solved faces carrying the exact selector/reference pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Active solved edges carrying the exact selector/reference pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_edges: Vec<EdgeId>,
}

/// Paired-locus frame nested under a dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignDimensionLocusPair {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Opaque u32 preceding the two locus references.
    pub opaque_index: u32,
    /// Byte offset of `opaque_index`.
    pub opaque_index_offset: u64,
    /// First typed sketch-geometry record.
    pub first_geometry_record_index: u32,
    /// Byte offset of the first geometry record index.
    pub first_geometry_reference_offset: u64,
    /// Source role code following the first geometry reference.
    pub first_role: u32,
    /// Byte offset of `first_role`.
    pub first_role_offset: u64,
    /// Second typed sketch-geometry record.
    pub second_geometry_record_index: u32,
    /// Byte offset of the second geometry record index.
    pub second_geometry_reference_offset: u64,
    /// Source role code following the second geometry reference.
    pub second_role: u32,
    /// Byte offset of `second_role`.
    pub second_role_offset: u64,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// Dimension frame with one null locus and one typed sketch-geometry locus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesignDimensionNullLocusPair {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Byte offset of the fixed zero record reference.
    pub null_reference_offset: u64,
    /// Role code attached to the null record reference.
    pub null_role: u32,
    /// Byte offset of `null_role`.
    pub null_role_offset: u64,
    /// Typed sketch-geometry record.
    pub geometry_record_index: u32,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Role code attached to the typed geometry record.
    pub geometry_role: u32,
    /// Byte offset of `geometry_role`.
    pub geometry_role_offset: u64,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// One typed geometry locus and its dimension-role code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesignDimensionLocus {
    /// Indexed sketch-point or sketch-curve record.
    pub geometry_record_index: u32,
    /// Byte offset of `geometry_record_index`.
    pub geometry_reference_offset: u64,
    /// Source role code following the geometry reference.
    pub role: u32,
    /// Byte offset of `role`.
    pub role_offset: u64,
}

/// Counted-locus frame nested under a dimensional parameter companion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignDimensionLocusGroup {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Companion record containing this frame.
    pub companion_record_index: u32,
    /// Byte offset of the indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Source indexed-record identity.
    pub record_index: u32,
    /// Byte length through the zero byte preceding the next indexed header.
    pub frame_length: u64,
    /// Ordered typed geometry loci.
    pub loci: Vec<DesignDimensionLocus>,
    /// Numeric design-entity suffix of the owning sketch.
    pub owner_reference: u32,
    /// Byte offset of `owner_reference`.
    pub owner_reference_offset: u64,
    /// Source role code following the owner reference.
    pub owner_role: u32,
    /// Byte offset of `owner_role`.
    pub owner_role_offset: u64,
    /// Source constraint-state mask.
    pub state: u32,
    /// Byte offset of `state`.
    pub state_offset: u64,
    /// Constraint kinds selected by `state`.
    pub constraint_kinds: Vec<SketchConstraintKind>,
    /// Bits in `state` outside the defined constraint mask.
    pub unknown_constraint_bits: u32,
    /// Ordered return geometry records.
    pub return_members: Vec<u32>,
    /// Byte offsets parallel to `return_members`.
    pub return_member_offsets: Vec<u64>,
    /// Dynamic class tag of the immediately following indexed record.
    pub next_class_tag: String,
    /// Identity of the immediately following indexed record.
    pub next_record_index: u32,
    /// Byte offset of the immediately following indexed record.
    pub next_byte_offset: u64,
}

/// Boolean result operation stored by an Extrude parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeOperation {
    /// Union the swept volume with the selected bodies.
    Join,
    /// Subtract the swept volume from the selected bodies.
    Cut,
    /// Retain the intersection of the swept volume and selected bodies.
    Intersect,
    /// Create an independent body.
    NewBody,
}

/// Extent form selected by the two fixed Extrude prologue enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeExtent {
    /// Travel a signed fixed distance on the first side of the profile.
    OneSidedDistance,
    /// Travel on the first side until reaching a selected face.
    OneSidedToFace,
    /// Travel independent fixed distances on both sides of the profile.
    TwoSidedDistance,
}

/// Starting support selected by the fixed Extrude prologue enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeStart {
    /// Start on the selected sketch's plane.
    ProfilePlane,
    /// Start on a parallel offset from the selected sketch's plane.
    OffsetProfilePlane,
    /// Start on a selected face.
    FromFace,
}

/// Indexed sketch or construction-operation record that scopes parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignParameterScope {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Source feature-family name.
    pub kind: String,
    /// Byte offset of the kind's UTF-16LE code units.
    pub kind_offset: u64,
    /// Extrude result operation from the fixed scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_operation: Option<DesignExtrudeOperation>,
    /// Byte offset of the Extrude operation enum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_operation_offset: Option<u64>,
    /// Extrude extent form from the fixed scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_extent: Option<DesignExtrudeExtent>,
    /// Byte offsets of the two u32 enums selecting the Extrude extent form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_extent_offsets: Option<[u64; 2]>,
    /// Whether a one-sided to-face extent travels opposite the profile normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_direction_reversed: Option<bool>,
    /// Byte offset of the Extrude direction-reversal Boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_direction_reversed_offset: Option<u64>,
    /// Extrude starting support from the fixed scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_start: Option<DesignExtrudeStart>,
    /// Byte offset of the u8 enum selecting the Extrude starting support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_start_offset: Option<u64>,
    /// One-based ordinal among scopes of the same feature family.
    pub feature_ordinal: u32,
    /// Byte offset of `feature_ordinal`.
    pub feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_state_id: Option<i64>,
    /// Byte offset of the encoded history-state identity or null sentinel.
    pub history_state_id_offset: u64,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity or null sentinel.
    pub previous_history_state_id_offset: u64,
    /// Byte offset of the ordered reference-table count.
    pub reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    pub reference_members: Vec<u32>,
    /// Byte offsets parallel to `reference_members`.
    pub reference_member_offsets: Vec<u64>,
    /// Profile operand carried by an Extrude scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_profile: Option<DesignExtrudeProfileOperand>,
    /// Full Design entity id of a sketch scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// Numeric suffix of `entity_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_suffix: Option<u64>,
    /// Byte offset of the sketch entity suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_reference_offset: Option<u64>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// Sketch-profile selection frame named by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignExtrudeProfileOperand {
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Asset UUID qualifying the selected Sketch reference.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    pub entity_id: String,
    /// Numeric suffix stored by the profile frame.
    pub entity_suffix: u64,
    /// Byte offset of the suffix's UTF-16LE code units.
    pub entity_reference_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
}

/// Counted selection group owned by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignExtrudeSelectionGroup {
    /// Globally unique deterministic identifier for this native group.
    pub id: String,
    /// Owning Extrude parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the counted member-run length.
    pub member_count_offset: u64,
    /// Ordered indexed selection-member records.
    pub members: Vec<u32>,
    /// Byte offsets parallel to `members`.
    pub member_offsets: Vec<u64>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    pub opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque finite f64 between the repeated u32 copies.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Boolean byte between the two nested-record references.
    pub variant: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
}

/// Semantic role of a counted Extrude operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeOperandRole {
    /// Existing bodies consumed by the Boolean operation.
    Bodies,
    /// Sketch profile swept by the Extrude.
    Profile,
    /// Faces used by profile-start or termination construction.
    Faces,
}

/// Semantic use of an ordered Extrude face-operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeFaceRole {
    /// Face supporting a selected-face start.
    Start,
    /// Face terminating a one-sided to-face extent.
    Termination,
}

/// Counted construction-operand group owned by a feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignConstructionOperandGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Position in the scope reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic primary class tag.
    pub class_tag: String,
    /// Byte offset of the member count.
    pub member_count_offset: u64,
    /// Ordered operand-record references.
    pub members: Vec<u32>,
    /// Ordered unresolved-edge records whose run terminates at this group's identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lost_edge_references: Vec<String>,
    /// Byte offsets parallel to `members`.
    pub member_offsets: Vec<u64>,
    /// Indexed identity-wrapper record.
    pub identity_record_index: u32,
    /// Byte offset of `identity_record_index`.
    pub identity_record_offset: u64,
    /// Source u64 role code.
    pub role: u64,
    /// Extrude-specific semantic role of `role`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_role: Option<DesignExtrudeOperandRole>,
    /// Start or termination role when `extrude_role` is `faces`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_face_role: Option<DesignExtrudeFaceRole>,
    /// Byte offset of `role`.
    pub role_offset: u64,
    /// Opaque repeated nonzero u32.
    pub opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque finite f64.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Boolean tail variant.
    pub variant: bool,
    /// Per-file dynamic paired class tag.
    pub paired_class_tag: String,
    /// Same-index paired-header byte offset.
    pub paired_byte_offset: u64,
}

/// Nested identity chain named by a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignConstructionOperandIdentity {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning operand-group record.
    pub group_record_index: u32,
    /// Ordered identity-wrapper indexed-record identities.
    pub wrapper_record_indices: Vec<u32>,
    /// Indexed-header byte offsets parallel to `wrapper_record_indices`.
    pub wrapper_byte_offsets: Vec<u64>,
    /// Per-file dynamic class tags parallel to `wrapper_record_indices`.
    pub wrapper_class_tags: Vec<String>,
    /// Indexed identity of the record physically following the wrappers.
    pub following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    pub following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    pub following_class_tag: String,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

/// Fixed-width persistent identity following a construction-operand identity chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignConstructionPersistentIdentity {
    /// Local persistent identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local identity.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local identity context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Identity of the indexed record immediately following this identity.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this identity.
    pub next_byte_offset: u64,
}

/// One radius assignment and its ordered edge group in a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignFilletRadiusGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning Fillet scope record.
    pub scope_record_index: u32,
    /// Position among construction-operand groups in scope-reference order.
    pub group_ordinal: u32,
    /// Counted construction-operand group carrying the edges.
    pub group_record_index: u32,
    /// Ordered edge-operand records assigned this radius.
    pub edge_operand_record_indices: Vec<u32>,
    /// Radius parameter record paired with this edge group.
    pub radius_parameter_record_index: u32,
    /// Tangency-weight parameter record paired with this edge group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangency_weight_parameter_record_index: Option<u32>,
}

/// One fixed-width member named by an Extrude selection group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignExtrudeSelectionMember {
    /// Globally unique deterministic identifier for this native member.
    pub id: String,
    /// Owning selection-group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Indexed-record identity named by the selection group.
    pub record_index: u32,
    /// Byte offset of the indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: String,
    /// Local persistent selection identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local selection identity.
    pub asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Sketch geometry carrying `local_id`, when it resolves uniquely in
    /// the selected Sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_geometry: Option<SketchRelationOperand>,
    /// Construction-operand identity chains that terminate at this member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operand_identity_ids: Vec<String>,
    /// Stable ASM history family carrying `local_id`, when family membership
    /// is unambiguous across every decoded state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historical_entity_kind: Option<AsmHistoricalEntityKind>,
    /// Stable ASM entity slot carrying `local_id` after record-revision
    /// identities are normalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historical_entity_ref: Option<i64>,
    /// ASM history states containing `local_id` in `historical_entity_kind`, in
    /// history arena order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_state_ids: Vec<i64>,
    /// Identity of the indexed record immediately following this member.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this member.
    pub next_byte_offset: u64,
}

/// Stable ASM entity family named by a Design persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AsmHistoricalEntityKind {
    /// Body topology slot.
    Body,
    /// Region topology slot.
    Region,
    /// Shell topology slot.
    Shell,
    /// Face topology slot.
    Face,
    /// Loop topology slot.
    Loop,
    /// Coedge topology slot.
    Coedge,
    /// Edge topology slot.
    Edge,
    /// Vertex topology slot.
    Vertex,
    /// Point carrier slot.
    Point,
    /// Surface carrier slot.
    Surface,
    /// Curve carrier slot.
    Curve,
    /// Parametric-curve carrier slot.
    Pcurve,
}

/// Edge-selection operand owned by a Fillet or Chamfer parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEdgeOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Indexed record containing the edge regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Standard two-side structure decoded from the recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_structure: Option<DesignEdgeRecipeStructure>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the preceding candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted or updated by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_boundary_edge_slots: Vec<i64>,
    /// Changed boundary-edge slots satisfying the recipe side clauses' face-loop
    /// edge counts when each side carries at most one entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipe_candidate_edge_slots: Vec<i64>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

/// Standard delimiter structure following an edge recipe's common prologue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEdgeRecipeStructure {
    /// Scalar before the first `-1` delimiter.
    pub root: i32,
    /// Two ordered side clauses.
    pub sides: [DesignEdgeRecipeSide; 2],
}

/// One delimiter-bounded side clause in a standard edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEdgeRecipeSide {
    /// Two words before the clause's first delimiter.
    pub header: [i32; 2],
    /// Scalar between the first and second clause delimiters.
    pub first: i32,
    /// Scalar between the second and third clause delimiters.
    pub second: i32,
    /// Optional scalar between the third and fourth clause delimiters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third: Option<i32>,
    /// Two-word prefix after the final scalar delimiter.
    pub payload_prefix: [i32; 2],
    /// Ordered eight-word payload entries.
    pub entries: Vec<DesignEdgeRecipeEntry>,
}

/// One eight-word topology entry in an edge-recipe side clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEdgeRecipeEntry {
    /// Side-local selector.
    pub selector: i32,
    /// Number of boundary edges on the referenced face loop.
    pub boundary_edge_count: NonZeroU32,
    /// Two ordered topology triplets.
    pub topology_triplets: [DesignEdgeRecipeTopologyTriplet; 2],
}

/// One three-word invariant in an edge-recipe entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEdgeRecipeTopologyTriplet {
    /// Equal positive first and third words.
    pub outer: NonZeroU32,
    /// Middle word, equal to `outer` or one less.
    pub middle: u32,
}

/// Face-selection operand owned by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignFaceOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by a face operand group.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Indexed record containing the face regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Byte offsets of the `[-1, -1, 2]` node openers declared by the program.
    pub recipe_node_offsets: Vec<u64>,
    /// Ordered nodes partitioning the program after its three-word header.
    pub recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_candidate_faces: Vec<FaceId>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

/// One length-delimited node in a face regeneration recipe program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignFaceRecipeNode {
    /// Byte offset of the node's `[-1, -1, 2]` opener.
    pub byte_offset: u64,
    /// Exclusive byte offset of the next node or the operand's following record.
    pub end_byte_offset: u64,
    /// Complete node words, including the three-word opener.
    pub program: Vec<i32>,
}

/// Local-to-model placement frame referenced by a Design sketch scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignSketchPlacement {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Parameter-scope record that references this placement.
    pub scope_record_index: u32,
    /// Full Design entity id of the placed sketch.
    pub entity_id: String,
    /// Numeric suffix of `entity_id`.
    pub entity_suffix: u64,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Row-major local-to-model affine transform.
    pub transform: [[f64; 4]; 4],
    /// Byte offset of the explicit 16-f64 matrix; absent for the compact identity form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_offset: Option<u64>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// Persistent-reference channel in the Design construction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistentReferenceKind {
    /// Reference identifies a persistent point.
    Point,
    /// Reference identifies the primary id of a persistent curve.
    CurvePrimary,
    /// Reference identifies the secondary id of a persistent curve.
    CurveSecondary,
}

/// One byte-stored persistent point or curve identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the persistent-reference field name in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the u64 value relative to `byte_offset`.
    pub value_offset: u32,
    /// Whether this reference identifies a persistent point or one end of a curve.
    pub kind: PersistentReferenceKind,
    /// Raw persistent point/curve identifier as stored in the `Design` construction stream.
    pub value: u64,
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LostEdgeReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the unresolved record's indexed header.
    pub record_byte_offset: u64,
    /// Byte offset of the unresolved record's three-byte class tag.
    pub class_tag_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag of the unresolved record.
    pub class_tag: String,
    /// Source `BulkStream` record index of the unresolved edge selection.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Byte offset of the `EDGE_REFERENCE_LOST` marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the indexed header immediately following this record.
    pub next_byte_offset: u64,
    /// Per-file dynamic class tag of the following indexed record.
    pub next_class_tag: String,
    /// Record index of the following indexed record.
    pub next_record_index: u32,
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignMaterialAssignment {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// ASM body key resolved through the Design body map.
    pub asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub asm_body_key_offset: u64,
    /// Numeric suffix of `entity_id`.
    pub entity_suffix: u64,
    /// Byte offset of the body-map entity suffix.
    pub entity_suffix_offset: u64,
    /// UTF-16 design-entity id.
    pub entity_id: String,
    /// Byte offset of the UTF-16 entity-id code units.
    pub entity_id_offset: u64,
    /// Visual asset GUID.
    pub visual_guid: String,
    /// Byte offset of the UTF-16 visual-GUID code units.
    pub visual_guid_offset: u64,
    /// Physical-material token, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_token: Option<String>,
    /// Byte offset of the UTF-16 physical token, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_token_offset: Option<u64>,
    /// Visual preset name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_preset: Option<String>,
    /// Byte offset of the UTF-16 preset name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_preset_offset: Option<u64>,
}

/// Design `MetaStream` object class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignObjectKind {
    /// Root Fusion document object.
    Fusion,
    /// A design body object.
    Body,
    /// A design component object.
    Component,
    /// A geometry-bearing object (points, curves, surfaces).
    Geometry,
    /// A sketch container object.
    Sketch,
    /// A parametric dimension/constraint object.
    Dimension,
    /// A scene/view object.
    Scene,
    /// An entity-tracking bookkeeping object.
    EntityTracking,
    /// A shared common-data object referenced by other object kinds.
    CommonData,
    /// A forward-compatible object class retained by its exact ASCII name.
    Other(String),
}

/// JSON configuration payload stored in a Fusion design-configuration entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignConfiguration {
    /// Stable identity derived from the ZIP entry name.
    pub id: String,
    /// Complete ZIP entry name used for native regeneration.
    pub entry_name: String,
    /// Native configuration entry family.
    pub kind: DesignConfigurationKind,
    /// Complete decoded JSON payload, including unrecognized fields.
    pub payload: serde_json::Value,
}

/// Native Fusion design-configuration entry family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignConfigurationKind {
    /// A `.dsgcfg` configuration table.
    Table,
    /// A `.dsgcfgrule` configuration rule.
    Rule,
}

/// One GUID-owned object-table record from the Design `MetaStream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignObject {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this object record in its Design `MetaStream`.
    pub byte_offset: u64,
    /// ASCII type name of this `MetaStream` object record.
    pub kind: DesignObjectKind,
    /// Design-entity ids owned by this object, in source `MetaStream` order; a count
    /// rather than a fixed-arity id list, so length varies per record.
    pub entity_ids: Vec<u64>,
    /// Byte offsets parallel to `entity_ids`.
    pub entity_id_offsets: Vec<u64>,
    /// This object's own GUID.
    pub self_guid: String,
    /// Byte offset of the self-GUID bytes in the Design `MetaStream`.
    pub self_guid_offset: u64,
    /// Number of zero delimiter bytes between the self GUID and the optional
    /// parent GUID.
    #[serde(default)]
    pub zero_run_length: u32,
    /// GUID of the owning object, when the source record carried a secondary GUID
    /// after the zero-run delimiter; `None` for root-level objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_guid: Option<String>,
    /// Byte offset of the parent-GUID bytes, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_guid_offset: Option<u64>,
    /// Trailing record-revision counter from the `MetaStream` record.
    pub revision: u32,
    /// Byte offset of `revision` in the Design `MetaStream`.
    pub revision_offset: u64,
}

/// Self-validating entity-bound header in the Design `BulkStream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEntityHeader {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this entity header in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Numeric suffix of the owning design-entity id (e.g. the `N` in `Body:N`).
    pub entity_suffix: u64,
    /// Full UTF-16LE-decoded design-entity id string for this header.
    pub entity_id: String,
    /// Source per-file dynamic three-digit ASCII class tag naming this header's record type.
    pub class_tag: String,
    /// Whether the flag-selected four-byte optional slot is present.
    pub optional_slot_present: bool,
    /// `MetaStream` object kind this header cross-references, when `optional_slot_present`
    /// resolved to a known `DesignObjectKind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<DesignObjectKind>,
    /// Index of an associated `BulkStream` record, when the header carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_reference: Option<u32>,
    /// Byte offset of `record_reference` in the Design `BulkStream`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_reference_offset: Option<u64>,
    /// Declared count of reference entries the header claims to own, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_reference_count: Option<u32>,
    /// Padded record-reference run owned by a sketch entity container.
    #[serde(default)]
    pub reference_indices: Vec<u32>,
    /// Byte offsets parallel to `reference_indices`.
    #[serde(default)]
    pub reference_offsets: Vec<u64>,
}

/// One indexed record header in the recursive Design `BulkStream` tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignRecordHeader {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this record within the recursive `BulkStream` tree.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Byte offset of this header within its Design `BulkStream`.
    pub byte_offset: u64,
}

/// Counted constraint relation owned by a sketch container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchRelation {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this relation record within the `BulkStream` tree.
    pub record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this relation's type.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the constraint mask relative to the record start.
    pub state_offset: u32,
    /// Numeric design-entity suffix of the sketch container that owns this relation.
    pub owner_reference: u32,
    /// Full Design entity id resolved from `owner_reference`.
    #[serde(default)]
    pub owner_entity_id: String,
    /// Nullable or role-specific references stored before the owner reference.
    #[serde(default)]
    pub auxiliary_references: Vec<u32>,
    /// Payload offsets parallel to `auxiliary_references`, relative to the record.
    #[serde(default)]
    pub auxiliary_reference_offsets: Vec<u32>,
    /// Record indices of the entities related by this relation.
    pub members: Vec<u32>,
    /// Member records resolved to typed sketch identities where available.
    #[serde(default)]
    pub resolved_members: Vec<SketchRelationOperand>,
    /// Payload offsets parallel to `members`, relative to the record.
    #[serde(default)]
    pub member_offsets: Vec<u32>,
    /// Payload offset of `owner_reference`, relative to the record.
    #[serde(default)]
    pub owner_reference_offset: u32,
    /// Source sketch-constraint bitmask.
    pub state: u32,
    /// Constraint kinds selected by `state`.
    #[serde(default)]
    pub constraint_kinds: Vec<SketchConstraintKind>,
    /// Bits in `state` outside the defined constraint mask.
    pub unknown_constraint_bits: u32,
    /// Record indices of entities returned or affected by this relation, distinct
    /// from `members`.
    pub return_members: Vec<u32>,
    /// Return-member records resolved to typed sketch identities where available.
    #[serde(default)]
    pub resolved_return_members: Vec<SketchRelationOperand>,
    /// Payload offsets parallel to `return_members`, relative to the record.
    #[serde(default)]
    pub return_member_offsets: Vec<u32>,
    /// Complete variable-width source record for native replay/write.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub raw_bytes: Vec<u8>,
}

/// One sketch-relation reference resolved against the indexed Design record graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchRelationOperand {
    /// A persistent sketch point.
    Point {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Persistent point identity stored by that record.
        persistent_id: u64,
    },
    /// A persistent sketch curve.
    Curve {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
        /// Primary persistent curve identity.
        primary_id: u64,
        /// Nullable secondary persistent curve identity.
        secondary_id: u64,
    },
    /// A referenced indexed record without point or curve identity fields.
    Record {
        /// Indexed Design record referenced by the relation.
        record_index: u32,
    },
}

/// One bit in a Fusion sketch-constraint state mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchConstraintKind {
    /// Points or endpoints occupy the same position.
    Coincident,
    /// Two line-bearing entities lie on one infinite line.
    Colinear,
    /// Circular entities share a center.
    Concentric,
    /// Line-bearing entities have equal length.
    EqualLength,
    /// Line-bearing entities have parallel directions.
    Parallel,
    /// Line-bearing entities meet at a right angle.
    Perpendicular,
    /// An entity is horizontal in sketch coordinates.
    Horizontal,
    /// An entity is vertical in sketch coordinates.
    Vertical,
    /// Two entities share a tangent direction at contact.
    Tangent,
    /// Two entities share curvature at contact.
    Curvature,
    /// Entities are symmetric about an axis.
    Symmetry,
    /// Entities have equal size.
    Equal,
    /// A point lies at an entity midpoint.
    Midpoint,
    /// Entities participate in a polygon relation.
    Polygon,
    /// Entities participate in a circular pattern.
    CircularPattern,
    /// Entities participate in a rectangular pattern.
    RectangularPattern,
}

/// One persistent 2D point in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchPoint {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this point record within the `BulkStream` tree.
    pub record_index: u32,
    /// Owning sketch entity derived from the relation records that use this point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this point's record type.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the first coordinate relative to the record start.
    pub coordinate_offset: u32,
    /// Optional persistent genesis identity carried ahead of the point identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Persistent Fusion identifier for this sketch point, stable across regeneration.
    pub persistent_id: u64,
    /// Record index of a paired/companion record (e.g. the owning sketch curve),
    /// when the source record carried one.
    pub paired_reference: u32,
    /// Sketch coordinates in millimetres.
    pub coordinates: Point2,
    /// Complete source record bytes for native replay and rewrite.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub raw_bytes: Vec<u8>,
}

/// Persistent identity pair attached to one source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchCurveIdentity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this identity record within the `BulkStream` tree.
    pub record_index: u32,
    /// Owning sketch entity derived from the relation records that use this curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_reference: Option<u32>,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Byte offset of this record within its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the fixed analytic geometry payload relative to the record start.
    pub geometry_offset: u32,
    /// Optional persistent genesis identity carried ahead of the curve identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_genesis: Option<u64>,
    /// Primary persistent identifier of the source sketch curve.
    pub primary_id: u64,
    /// Secondary persistent identifier of the source sketch curve (e.g. its
    /// complementary endpoint or paired-curve identity).
    pub secondary_id: u64,
    /// Exact analytic geometry carried by this sketch-curve record, when the
    /// decoder recovered one; `None` when the geometry subtype was not decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<SketchCurveGeometry>,
}

/// Exact analytic geometry carried by a source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchCurveGeometry {
    /// A straight line segment.
    Line {
        /// Start point in sketch space, millimetres.
        start: Point3,
        /// End point in sketch space, millimetres.
        end: Point3,
        /// Unit direction vector from `start` to `end`.
        direction: Vector3,
        /// Unit normal of the sketch plane the line lies in.
        normal: Vector3,
    },
    /// A circular arc.
    Arc {
        /// Arc center in sketch space, millimetres.
        center: Point3,
        /// Unit normal of the sketch plane the arc lies in.
        normal: Vector3,
        /// Unit vector marking the zero-angle direction for `start_angle`/`end_angle`.
        reference_direction: Vector3,
        /// Arc radius in millimetres.
        radius: f64,
        /// Start angle in radians, measured from `reference_direction`.
        start_angle: f64,
        /// End angle in radians, measured from `reference_direction`.
        end_angle: f64,
    },
    /// A NURBS (procedural spline) curve.
    Nurbs {
        /// Record index of the underlying carrier geometry, when the NURBS record
        /// references one; `None` when the control data is self-contained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carrier_reference: Option<u64>,
        /// Source per-file dynamic three-digit ASCII class tag naming the NURBS subtype.
        subtype_class_tag: String,
        /// Record index of the NURBS subtype record.
        subtype_record_index: u32,
        /// Polynomial degree of the curve.
        degree: u32,
        /// Source fit tolerance used when the curve was fitted, in millimetres.
        fit_tolerance: f64,
        /// Width in scalars of each control-point record as stored in the source
        /// (control point components plus weight, before decoding into `control_points`/`weights`).
        scalar_width: u32,
        /// Knot vector, non-decreasing, length `control_points.len() + degree + 1`.
        knots: Vec<f64>,
        /// Per-control-point rational weights, parallel to `control_points`.
        weights: Vec<f64>,
        /// Control points in sketch space, millimetres, parallel to `weights`.
        control_points: Vec<Point3>,
    },
}

/// One member of the Design `BulkStream` `BodiesRoot` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignBodyMember {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this member's leading presence byte in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Numeric suffix of this body's design-entity id.
    pub entity_suffix: u64,
    /// Source per-member flag word from the `BodiesRoot` list entry.
    pub flags: u16,
}

/// Triplicated axis-aligned body bounds cached in the Design stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignBodyBounds {
    /// Globally unique deterministic identifier for this native record set.
    pub id: String,
    /// Numeric suffix of the owning Design body entity.
    pub entity_suffix: u64,
    /// Byte offset of the owning Design entity header.
    pub entity_byte_offset: u64,
    /// Three consecutive indexed record identities carrying the cache.
    pub record_indices: [u32; 3],
    /// Indexed-header byte offsets parallel to `record_indices`.
    pub record_byte_offsets: [u64; 3],
    /// First f64 byte of each repeated sextuple.
    pub value_byte_offsets: [u64; 3],
    /// Design BREP body-map pairs carrying this entity suffix, in stream order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_binding_ids: Vec<String>,
    /// Maximum model-space corner in millimetres.
    pub maximum: Point3,
    /// Minimum model-space corner in millimetres.
    pub minimum: Point3,
}

/// One ordered pair in a Design `BulkStream` BREP body-map record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignBodyBinding {
    /// Globally unique deterministic identifier for this native map entry.
    pub id: String,
    /// Design `BulkStream` ZIP entry containing the map.
    pub stream: String,
    /// Number of pairs in the enclosing body map.
    pub pair_count: u32,
    /// Zero-based position in the enclosing body map.
    pub pair_ordinal: u32,
    /// ASM body key stored by this pair.
    pub asm_body_key: u64,
    /// Byte offset of `asm_body_key` within `stream`.
    pub asm_body_key_offset: u64,
    /// Numeric Design entity suffix stored by this pair.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within `stream`.
    pub entity_suffix_offset: u64,
    /// Basename of the BREP blob whose body namespace contains the key.
    pub blob_name: String,
    /// Byte offset of the UTF-16LE `blob_name` code units within `stream`.
    pub blob_name_offset: u64,
    /// Solved body when this pair targets the selected active BREP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyId>,
}

/// Design browser-node visibility joined to one solved ASM body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BodyVisibility {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved B-rep body controlled by the browser node.
    pub body: BodyId,
    /// Design `BulkStream` ZIP entry containing the browser node.
    pub stream: String,
    /// Byte offset of the browser node's hidden flag within `stream`.
    pub byte_offset: u64,
    /// Byte offset of the joined body-map ASM key within `stream`.
    pub asm_body_key_offset: u64,
    /// ASM body key used by the BREP body-map join.
    pub asm_body_key: u64,
    /// Numeric Design entity suffix stored by both joined records.
    pub entity_suffix: u64,
    /// Display visibility after inverting the native hidden flag.
    pub visible: bool,
}

/// Native Design-join key stored on one ASM body record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BodyNativeKey {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Solved body carrying the key.
    pub body: BodyId,
    /// Source SAB body record index.
    pub record_index: u32,
    /// Non-negative Design-join key; absence is the native `-1` sub-body sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asm_body_key: Option<u64>,
}

/// Native rotation, reflection, and shear classifications on an ASM transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// One entity in the Fusion ACT change-tracking table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActEntity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Index of this entity's `ACTTable` entry within the ACT `BulkStream`.
    pub record_index: u32,
    /// Byte offset of the table-entry record index, when this entity is in `ACTTable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_record_index_offset: Option<u64>,
    /// Byte offset of the channel-group record index, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_record_index_offset: Option<u64>,
    /// UTF-16LE-decoded design-entity id this table entry tracks.
    pub entity_id: String,
    /// Byte offset of the table-entry UTF-16 entity-id code units, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_entity_id_offset: Option<u64>,
    /// Byte offset of the channel-group UTF-16 entity-id code units, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_entity_id_offset: Option<u64>,
    /// Whether this entity is currently present in the `ACTTable`, as opposed to
    /// referenced only by a channel-group record.
    pub in_table: bool,
    /// Source per-file dynamic three-digit ASCII class tag of this entity's channel-group
    /// record, when it owns one; `None` for table-only entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_class_tag: Option<String>,
    /// Named channel/GUID pairs from this entity's channel-group record; each GUID is a
    /// change-version handle, not a visibility or suppression flag.
    #[serde(default)]
    pub channels: BTreeMap<String, String>,
    /// Byte offsets of UTF-16 GUID code units, keyed parallel to `channels`.
    #[serde(default)]
    pub channel_guid_offsets: BTreeMap<String, u64>,
}

/// One GUID in the ordered ACT stream-wide asset/change-version pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActGuid {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this GUID's UTF-16 length prefix in the ACT `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the UTF-16 GUID code units in the ACT `BulkStream`.
    pub guid_offset: u64,
    /// Position of this GUID in the pool, in source stream order; pool position does
    /// not assign one GUID to a single `ACTTable` entry.
    pub ordinal: u32,
    /// The pooled GUID string.
    pub guid: String,
}

/// ACT link from the document root entity to the instance/component registries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActRootComponent {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of this record in the ACT `BulkStream`.
    pub byte_offset: u64,
    /// Index of this record within the ACT `BulkStream`.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub class_tag: String,
    /// Record index of the instance registry root.
    pub instance_root_record: u32,
    /// Byte offset of `instance_root_record`.
    pub instance_root_record_offset: u64,
    /// Record index of the components registry root.
    pub components_root_record: u32,
    /// Byte offset of `components_root_record`.
    pub components_root_record_offset: u64,
    /// Source counter/registry flag; 0 and 1 are both valid.
    pub registry_flag: u32,
    /// Byte offset of `registry_flag`.
    pub registry_flag_offset: u64,
    /// UTF-16LE-decoded design-entity id of the document root entity.
    pub entity_id: String,
    /// Byte offset of the UTF-16 `entity_id` code units.
    pub entity_id_offset: u64,
    /// Document display name as stored alongside this root-component link.
    pub display_name: String,
    /// Byte offset of the UTF-16 `display_name` code units.
    pub display_name_offset: u64,
}
