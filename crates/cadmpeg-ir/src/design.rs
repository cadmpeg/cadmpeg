// SPDX-License-Identifier: Apache-2.0
//! Parametric-design entities and their links to the solved B-rep.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::attributes::AttributeTarget;
use crate::ids::CoedgeId;
use crate::math::{Point2, Point3, Vector3};
use crate::provenance::EntityMeta;

/// Provenance link from a solved B-rep coedge to its source sketch curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchCurveLink {
    pub coedge: CoedgeId,
    pub sketch_curve_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_reference: Option<i64>,
    pub role: i64,
    pub closure: i64,
    pub meta: EntityMeta,
}

/// Persistent Fusion design identifier attached to a solved B-rep entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentDesignLink {
    pub target: AttributeTarget,
    pub design_id: String,
    pub ordinal: u32,
    pub is_current: bool,
    pub meta: EntityMeta,
}

/// Design BulkStream regeneration-recipe family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionRecipeKind {
    Body,
    Face,
    BoundedFace,
    Edge,
    Vertex,
}

/// One source-framed parametric regeneration recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConstructionRecipe {
    pub kind: ConstructionRecipeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id: Option<String>,
    pub recipe_index: u32,
    pub record_index: i32,
    pub meta: EntityMeta,
}

/// Persistent-reference channel in the Design construction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistentReferenceKind {
    Point,
    CurvePrimary,
    CurveSecondary,
}

/// One byte-stored persistent point or curve identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersistentReference {
    pub kind: PersistentReferenceKind,
    pub value: u64,
    pub meta: EntityMeta,
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LostEdgeReference {
    pub class_tag: String,
    pub record_index: u32,
    pub meta: EntityMeta,
}

/// Design MetaStream object class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesignObjectKind {
    Fusion,
    Body,
    Component,
    Geometry,
    Sketch,
    Dimension,
    Scene,
    EntityTracking,
    CommonData,
}

/// One GUID-owned object-table record from the Design MetaStream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignObject {
    pub kind: DesignObjectKind,
    pub entity_ids: Vec<u64>,
    pub self_guid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_guid: Option<String>,
    pub revision: u32,
    pub meta: EntityMeta,
}

/// Self-validating entity-bound header in the Design BulkStream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignEntityHeader {
    pub entity_suffix: u64,
    pub entity_id: String,
    pub class_tag: String,
    /// Whether the flag-selected four-byte optional slot is present.
    pub optional_slot_present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<DesignObjectKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_reference: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_reference_count: Option<u32>,
    /// Padded record-reference run owned by a sketch entity container.
    #[serde(default)]
    pub reference_indices: Vec<u32>,
    pub meta: EntityMeta,
}

/// One indexed record header in the recursive Design BulkStream tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignRecordHeader {
    pub record_index: u32,
    pub class_tag: String,
    pub meta: EntityMeta,
}

/// Bidirectional two-member relation owned by a sketch container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchRelation {
    pub record_index: u32,
    pub class_tag: String,
    pub owner_reference: u32,
    pub members: Vec<u32>,
    pub state: u32,
    pub return_members: Vec<u32>,
    /// Complete 101-byte source record for native replay/write.
    pub raw_bytes: Vec<u8>,
    pub meta: EntityMeta,
}

/// One persistent 2D point in a Fusion sketch coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchPoint {
    pub record_index: u32,
    pub class_tag: String,
    pub persistent_id: u64,
    pub paired_reference: u32,
    /// Sketch coordinates in millimetres.
    pub coordinates: Point2,
    pub meta: EntityMeta,
}

/// Persistent identity pair attached to one source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchCurveIdentity {
    pub record_index: u32,
    pub class_tag: String,
    pub primary_id: u64,
    pub secondary_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<SketchCurveGeometry>,
    pub meta: EntityMeta,
}

/// Exact analytic geometry carried by a source sketch-curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchCurveGeometry {
    Line {
        start: Point3,
        end: Point3,
        direction: Vector3,
        normal: Vector3,
    },
    Arc {
        center: Point3,
        normal: Vector3,
        reference_direction: Vector3,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
}

/// One member of the Design BulkStream `BodiesRoot` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignBodyMember {
    pub entity_suffix: u64,
    pub flags: u16,
    pub meta: EntityMeta,
}

/// One entity in the Fusion ACT change-tracking table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActEntity {
    pub record_index: u32,
    pub entity_id: String,
    pub in_table: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_class_tag: Option<String>,
    #[serde(default)]
    pub channels: BTreeMap<String, String>,
    pub meta: EntityMeta,
}

/// One GUID in the ordered ACT stream-wide asset/change-version pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActGuid {
    pub ordinal: u32,
    pub guid: String,
    pub meta: EntityMeta,
}

/// ACT link from the document root entity to the instance/component registries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActRootComponent {
    pub record_index: u32,
    pub class_tag: String,
    pub instance_root_record: u32,
    pub components_root_record: u32,
    /// Source counter/registry flag; 0 and 1 are both valid.
    pub registry_flag: u32,
    pub entity_id: String,
    pub display_name: String,
    pub meta: EntityMeta,
}
