// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` parametric construction-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A named parametric-model variant (e.g. CAD "configuration") with its own
/// material and property overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Configuration {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning feature-history record id.
    pub parent: String,
    /// Position in the source configuration list.
    #[serde(default)]
    pub ordinal: u32,
    /// Numeric key used by configuration-scoped container sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_index: Option<u32>,
    /// Source configuration name.
    pub name: String,
    /// Material assigned in this configuration, when overridden; `None` when the
    /// configuration inherits the part's default material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Source custom-property name/value pairs local to this configuration.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

fn default_feature_xml_tag() -> String {
    "Feature".into()
}

/// One parametric construction-history feature (e.g. an extrude or fillet operation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning feature-history record id.
    pub parent: String,
    /// XML element name carrying this feature record.
    #[serde(default = "default_feature_xml_tag")]
    pub xml_tag: String,
    /// Native record id of the containing feature element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_parent: Option<String>,
    /// Native identifier of this feature, when the source assigned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Native identifier of this feature's parent in the construction tree, when
    /// the source recorded parent/child feature dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_source_id: Option<String>,
    /// Position of this feature in the construction-history timeline, in
    /// regeneration order.
    pub ordinal: u32,
    /// Feature display name.
    pub name: String,
    /// Native feature-type tag (e.g. `"Extrude"`, `"Fillet"`).
    pub kind: String,
    /// Whether this feature is suppressed and excluded from regeneration.
    #[serde(default)]
    pub suppressed: bool,
    /// Source parametric input values keyed by parameter name.
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    /// Source attributes on each named dimension, excluding its `Name` key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dimension_properties: BTreeMap<String, BTreeMap<String, String>>,
    /// Source custom-property name/value pairs local to this feature.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    /// Text content of a native leaf feature element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Source order of dimensions, nested feature nodes, and text content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<FeatureContent>,
}

/// One ordered item inside a native feature XML element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FeatureContent {
    /// Named dimension child.
    Dimension(String),
    /// Native record id of a nested feature child.
    Feature(String),
    /// Non-whitespace text content.
    Text(String),
}

/// One ordered item inside the native `Keywords` root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HistoryContent {
    /// Native configuration record id.
    Configuration(String),
    /// Native top-level feature record id.
    Feature(String),
    /// Non-whitespace root text content.
    Text(String),
}

/// The full parametric construction-history timeline for a part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureHistory {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Source part display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_name: Option<String>,
    /// Source attributes on the `Keywords` root, excluding its `Name` key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Source order of configurations, top-level features, and root text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<HistoryContent>,
    /// Named parametric-model variants defined on this part.
    #[serde(default)]
    pub configurations: Vec<Configuration>,
    /// Ordered construction-history features, in regeneration order.
    #[serde(default)]
    pub features: Vec<Feature>,
}

/// Native feature-input stream retained for parametric replay and rewrite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputLane {
    /// Stable source-derived identifier for this feature-input record.
    pub id: String,
    /// Configuration this input lane applies to, when the source scoped inputs
    /// per configuration; `None` when the lane applies to all configurations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Complete native feature-input byte stream, retained undecoded for
    /// parametric replay and native rewrite.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub native_payload: Vec<u8>,
    /// Class declarations used by object instances in this lane.
    #[serde(default)]
    pub classes: Vec<FeatureInputClass>,
    /// Serialized object names in this lane.
    #[serde(default)]
    pub names: Vec<FeatureInputName>,
    /// Named scalar values in this lane.
    #[serde(default)]
    pub scalars: Vec<FeatureInputScalar>,
    /// Native entity-reference cells in byte order.
    #[serde(default)]
    pub references: Vec<FeatureInputReference>,
    /// Typed sketch-entity markers located within `native_payload`.
    #[serde(default)]
    pub sketch_entities: Vec<SketchInputEntity>,
}

/// One native entity-reference cell in a feature-input stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputReference {
    /// Globally unique deterministic identifier for this cell.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Native history feature enclosing this cell, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_ref: Option<String>,
    /// Position among reference cells in stream order.
    pub ordinal: u32,
    /// Byte offset of the reference cell.
    pub offset: u64,
    /// Native reference-cell family.
    pub kind: FeatureInputOperandKind,
    /// Local object index carried by the cell.
    pub object_index: u16,
}

/// One serialized UTF-16 object name in a feature-input stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputName {
    /// Globally unique deterministic identifier for this name record.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among serialized names in stream order.
    pub ordinal: u32,
    /// Byte offset of the name marker.
    pub offset: u64,
    /// Decoded object name.
    pub value: String,
}

/// One named scalar serialized in native SI units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputScalar {
    /// Globally unique deterministic identifier for this scalar record.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among named scalars in stream order.
    pub ordinal: u32,
    /// Byte offset of the little-endian f64 value.
    pub offset: u64,
    /// Native object identifier carried by the scalar record.
    pub object_id: u32,
    /// Name record attached to this scalar.
    pub name: String,
    /// Scalar value in native SI units.
    pub value: f64,
    /// Function of this scalar in the dimension record.
    pub role: FeatureInputScalarRole,
    /// Local sketch-entity indices used as dimension operands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_indices: Vec<u16>,
    /// Typed native operand cells attached to this scalar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operands: Vec<FeatureInputOperand>,
}

/// One native entity-reference cell attached to a feature-input scalar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputOperand {
    /// Byte offset of the reference cell within the feature-input stream.
    pub offset: u64,
    /// Reference-cell record at this byte offset.
    pub reference_ref: String,
    /// Native reference-cell family.
    pub kind: FeatureInputOperandKind,
    /// Local entity index carried by the cell.
    pub entity_index: u16,
    /// Resolved sketch-input entity in the same feature object, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_ref: Option<String>,
}

/// Native feature-input entity-reference cell family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureInputOperandKind {
    /// `d6 80` reference cell.
    D6,
    /// `e1 80` reference cell.
    E1,
}

/// Function of a named scalar in its dimension record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureInputScalarRole {
    /// Value consumed during model regeneration.
    Driving,
    /// Dimension-label placement or display value.
    Display,
    /// Scalar from a different native record layout.
    Native,
}

/// One class declaration in a native feature-input stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputClass {
    /// Globally unique deterministic identifier for this declaration.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among class declarations in stream order.
    pub ordinal: u32,
    /// Byte offset of the `ff ff 01 00` declaration marker.
    pub offset: u64,
    /// Declared native class name.
    pub name: String,
    /// Design-intent role of this class.
    #[serde(default)]
    pub role: FeatureInputClassRole,
}

/// Design-intent role declared by a feature-input class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureInputClassRole {
    /// Modeling operation or construction feature.
    Feature,
    /// Sketch container.
    Sketch,
    /// Sketch geometry handle.
    SketchEntity,
    /// Geometric sketch relation.
    SketchConstraint,
    /// Driving or driven dimension.
    Dimension,
    /// Scalar feature parameter.
    Parameter,
    /// Reference to another model object.
    Reference,
    /// Supporting serialization object.
    Auxiliary,
    /// Class with no typed role.
    #[default]
    Native,
}

/// One typed sketch-entity marker inside a native feature-input stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchInputEntity {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position of this marker within the owning `FeatureInputLane`, in stream order.
    pub ordinal: u32,
    /// Byte offset of this marker within `FeatureInputLane::native_payload`.
    pub offset: u64,
    /// Sketch-local object identifier preceding the marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<u32>,
    /// Sketch-entity kind this marker identifies.
    pub kind: SketchInputKind,
}

/// Kind of sketch entity referenced by a native feature-input marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchInputKind {
    /// A sketch point.
    Point,
    /// A sketch line or circle from the shared native family.
    #[serde(alias = "curve")]
    LineOrCircle,
    /// A sketch arc.
    Arc,
    /// A sketch point bound by a geometric constraint.
    ConstrainedPoint,
    /// A native code not in the known vocabulary, preserved verbatim.
    Native(u32),
}

impl SketchInputKind {
    /// Maps a native sketch-entity type code to its typed kind, falling back to
    /// [`SketchInputKind::Native`] for unrecognized codes.
    pub fn from_native_code(code: u32) -> Self {
        match code {
            0 => Self::Point,
            1 => Self::LineOrCircle,
            2 => Self::Arc,
            3 => Self::ConstrainedPoint,
            value => Self::Native(value),
        }
    }

    /// Returns the native sketch-entity type code for this kind, the inverse of
    /// [`SketchInputKind::from_native_code`].
    pub fn native_code(self) -> u32 {
        match self {
            Self::Point => 0,
            Self::LineOrCircle => 1,
            Self::Arc => 2,
            Self::ConstrainedPoint => 3,
            Self::Native(value) => value,
        }
    }
}
