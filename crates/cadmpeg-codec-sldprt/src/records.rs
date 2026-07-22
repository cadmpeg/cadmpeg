// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` parametric construction-history records.
#![deny(clippy::disallowed_methods)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One semantic product-manufacturing dimension from `PMISemanticDataDB`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PmiDimension {
    /// Globally unique source-derived record id.
    pub id: String,
    /// Source block containing this record.
    pub parent: String,
    /// Byte offset of the `MessagePack` map within the decompressed block.
    pub offset: u64,
    /// `UnQLite` record key.
    pub guid: String,
    /// CAD dimension reference, such as `D1@Sketch4`.
    pub cad_text: String,
    /// Native PMI dimension subtype.
    pub subtype: String,
    /// Stored dimension value.
    pub value: f64,
    /// Byte offset of the big-endian `f64` value.
    pub value_offset: u64,
    /// Display precision.
    pub precision: i64,
    /// Byte offset of the `MessagePack` precision value.
    pub precision_offset: u64,
    /// Native formatted dimension text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,
    /// Byte offset of the formatted-text bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_text_offset: Option<u64>,
    /// Basic-dimension flag.
    pub basic: bool,
    /// Byte offset of the basic flag.
    pub basic_offset: u64,
    /// Inspection-dimension flag.
    pub inspection: bool,
    /// Byte offset of the inspection flag.
    pub inspection_offset: u64,
    /// Reference-only flag.
    pub reference_only: bool,
    /// Byte offset of the reference-only flag.
    pub reference_only_offset: u64,
}

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
    /// Serialized feature-input object class owning this feature, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_class: Option<String>,
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
    /// Relation-class declarations bound to their attached scalar records.
    #[serde(default)]
    pub relation_bindings: Vec<FeatureInputRelationBinding>,
    /// Compact relation instances grouped by feature and operand identity.
    #[serde(default)]
    pub relation_instances: Vec<FeatureInputRelationInstance>,
    /// Compact body-selection vectors owned by feature objects in this lane.
    #[serde(default)]
    pub body_selections: Vec<FeatureInputBodySelection>,
    /// Compact edge-selection vectors owned by feature objects in this lane.
    #[serde(default)]
    pub edge_selections: Vec<FeatureInputEdgeSelection>,
    /// Compact surface-component selections owned by feature objects in this lane.
    #[serde(default)]
    pub surface_selections: Vec<FeatureInputSurfaceSelection>,
    /// Persistent identities of surfaces produced by regenerated features.
    #[serde(default)]
    pub generated_surface_identities: Vec<FeatureInputGeneratedSurfaceIdentity>,
    /// Native entity-reference cells in byte order.
    #[serde(default)]
    pub references: Vec<FeatureInputReference>,
    /// Typed sketch-entity markers located within `native_payload`.
    #[serde(default)]
    pub sketch_entities: Vec<SketchInputEntity>,
}

/// One compact feature-local body-selection vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputBodySelection {
    /// Globally unique deterministic identifier for this vector.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among compact body-selection vectors in stream order.
    pub ordinal: u32,
    /// Byte offset of the schema word opening the vector.
    pub offset: u64,
    /// Feature-input name record owning this vector.
    pub object_name_ref: String,
    /// Native history feature owning this vector.
    pub feature_ref: String,
    /// Ordered feature-local body identifiers.
    pub local_body_ids: Vec<u32>,
    /// Ordered body-state records stored before the selection vector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_state_ids: Vec<u32>,
    /// Retention mode carried by the delete-body data record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<cadmpeg_ir::features::BodyRetentionMode>,
}

/// One compact feature-local edge-selection vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputEdgeSelection {
    /// Globally unique deterministic identifier for this vector.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among compact edge-selection vectors in stream order.
    pub ordinal: u32,
    /// Byte offset of the vector marker.
    pub offset: u64,
    /// Feature-input name record owning this vector.
    pub object_name_ref: String,
    /// Native history feature owning this vector.
    pub feature_ref: String,
    /// Ordered feature-local edge identifiers.
    pub local_edge_ids: Vec<u32>,
    /// Complete typed path entries when this is an entry-form vector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<FeatureInputComponentPathEntry>,
    /// Ordered history features traversed by the persistent edge path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_feature_refs: Vec<String>,
    /// History feature owning the terminal edge component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_feature_ref: Option<String>,
}

/// One compact feature-local surface-component selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputSurfaceSelection {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among surface selections in stream order.
    pub ordinal: u32,
    /// Byte offset of the vector marker.
    pub offset: u64,
    /// Feature-input name record owning this selection.
    pub object_name_ref: String,
    /// Native history feature owning this selection.
    pub feature_ref: String,
    /// Ordered native history features traversed by the persistent surface path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_feature_refs: Vec<String>,
    /// Native history feature owning the terminal face component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_feature_ref: Option<String>,
    /// Ordered typed entries in the persistent surface-component path.
    #[serde(default)]
    pub components: Vec<FeatureInputComponentPathEntry>,
}

/// One persistent identity of a surface produced by a regenerated feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputGeneratedSurfaceIdentity {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among generated surface identities in stream order.
    pub ordinal: u32,
    /// Byte offset of the first component type signature.
    pub offset: u64,
    /// Four-byte serialized surface identity type family.
    pub type_prefix: [u8; 4],
    /// Source identifier of the feature that produced the terminal surface.
    pub feature_source_id: u32,
    /// Opaque feature-local identity of the terminal surface.
    pub local_identity: u32,
    /// Ordered typed entries in the persistent generated-surface path.
    #[serde(default)]
    pub components: Vec<FeatureInputComponentPathEntry>,
}

/// One typed node in a persistent feature-input component path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputComponentPathEntry {
    /// Serialized component instance tag; absent on anonymous path nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<u16>,
    /// Twelve-byte serialized component type identity.
    pub type_signature: [u8; 12],
    /// Feature-local identifier carried by terminal selection nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<u32>,
}

/// A declared sketch-relation family and its attached scalar record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputRelationBinding {
    /// Globally unique deterministic identifier for this binding.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among relation bindings in stream order.
    pub ordinal: u32,
    /// Byte offset of the relation class declaration.
    pub offset: u64,
    /// Declared class record.
    pub class_ref: String,
    /// Native relation family.
    pub family: FeatureInputRelationFamily,
    /// Scalar record attached to the declaration.
    pub scalar_ref: String,
    /// Native history feature owning the relation, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_ref: Option<String>,
}

/// One compact sketch-relation instance represented by related scalar records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputRelationInstance {
    /// Globally unique deterministic identifier for this relation instance.
    pub id: String,
    /// Owning feature-input lane record id.
    pub parent: String,
    /// Position among relation instances in scalar stream order.
    pub ordinal: u32,
    /// First participating scalar's byte offset.
    pub offset: u64,
    /// Native relation family.
    pub family: FeatureInputRelationFamily,
    /// Class declaration defining the relation family.
    pub class_ref: String,
    /// Native sketch feature owning the relation.
    pub feature_ref: String,
    /// Scalar records carrying measured and target values.
    pub scalar_refs: Vec<String>,
    /// Unique driving scalar carrying the target parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_scalar_ref: Option<String>,
    /// Unique display-role scalar attached to the relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_scalar_ref: Option<String>,
    /// Operand cells shared by the participating scalar records.
    pub operands: Vec<FeatureInputOperand>,
}

/// Native sketch-relation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureInputRelationFamily {
    /// Diameter of one circular sketch entity.
    CircleDiameter,
    /// Distance between two line loci.
    LineLineDistance,
    /// Distance between two point loci.
    PointPointDistance,
    /// Distance between a point locus and a line locus.
    PointLineDistance,
    /// Horizontal distance between two point loci.
    PointPointHorizontalDistance,
    /// Vertical distance between two point loci.
    PointPointVerticalDistance,
    /// Angle between two entity loci.
    Angle,
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
    /// Native object identifier stored after the UTF-16 name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u32>,
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
    /// Native history feature enclosing this scalar, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_ref: Option<String>,
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
    /// Other two-byte reference-cell tag, stored as a little-endian u16.
    Native(u16),
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
    /// Native history feature whose serialized object interval contains this marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_ref: Option<String>,
    /// Position of this marker within the owning `FeatureInputLane`, in stream order.
    pub ordinal: u32,
    /// Byte offset of this marker within `FeatureInputLane::native_payload`.
    pub offset: u64,
    /// Feature-local object index stored immediately before the marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_index: Option<u32>,
    /// Feature-local object identifier stored in the marker trailer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<u32>,
    /// Sketch-entity kind this marker identifies.
    pub kind: SketchInputKind,
    /// Finite little-endian state scalar stored 48 bytes after the marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_value: Option<f64>,
    /// Two little-endian coordinate fields stored by geometry-handle marker families, in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinates_m: Option<[f64; 2]>,
    /// Resolved marker-local links carried by the reference-bearing layout.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<SketchInputLink>,
    /// Selector stored beside `links` in the reference-bearing layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_selector: Option<u16>,
}

/// One marker-local reference resolved within its owning feature object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SketchInputLink {
    /// Feature-local object identifier stored in the marker payload.
    pub local_id: u16,
    /// Typed sketch-input marker with this local identifier.
    pub entity_ref: String,
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
    /// A sketch relation handle.
    Relation(SketchRelationKind),
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

    /// Maps a marker code using the marker layout to separate geometry handles
    /// from relation handles that reuse codes `1..3`.
    pub fn from_native_code_and_layout(code: u32, coordinate_bearing: bool) -> Self {
        if code == 0 || (coordinate_bearing && code <= 3) {
            return Self::from_native_code(code);
        }
        SketchRelationKind::from_native_code(code).map_or(Self::Native(code), Self::Relation)
    }

    /// Returns the native sketch-entity type code for this kind, the inverse of
    /// [`SketchInputKind::from_native_code`].
    pub fn native_code(self) -> u32 {
        match self {
            Self::Point => 0,
            Self::LineOrCircle => 1,
            Self::Arc => 2,
            Self::ConstrainedPoint => 3,
            Self::Relation(relation) => relation.native_code(),
            Self::Native(value) => value,
        }
    }

    /// Whether this marker owns constraint semantics that require a neutral
    /// projection. Dimensional marker handles are operands of scalar-bearing
    /// relation instances and do not independently encode a constraint.
    pub fn owns_constraint(self) -> bool {
        match self {
            Self::Relation(
                SketchRelationKind::Distance
                | SketchRelationKind::Angle
                | SketchRelationKind::Radius
                | SketchRelationKind::Diameter,
            ) => false,
            Self::Relation(_) | Self::Native(_) => true,
            Self::Point | Self::LineOrCircle | Self::Arc | Self::ConstrainedPoint => false,
        }
    }
}

/// Relation kind carried by a non-coordinate sketch marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchRelationKind {
    /// Linear distance.
    Distance,
    /// Angular distance.
    Angle,
    /// Radius dimension.
    Radius,
    /// Horizontal entity or point alignment.
    Horizontal,
    /// Vertical entity or point alignment.
    Vertical,
    /// Tangency.
    Tangent,
    /// Parallelism.
    Parallel,
    /// Perpendicularity.
    Perpendicular,
    /// Point-on-entity coincidence.
    Coincident,
    /// Shared center.
    Concentric,
    /// Symmetry about a centerline.
    Symmetric,
    /// Midpoint incidence.
    Midpoint,
    /// Intersection incidence.
    AtIntersection,
    /// Equal length or radius.
    Equal,
    /// Diameter dimension.
    Diameter,
    /// Offset-edge relation.
    OffsetEdge,
    /// Fixed geometry.
    Fixed,
    /// Arc angle fixed at 90 degrees.
    ArcAngle90,
    /// Arc angle fixed at 180 degrees.
    ArcAngle180,
    /// Arc angle fixed at 270 degrees.
    ArcAngle270,
    /// Arc constrained to the top cardinal position.
    ArcAngleTop,
    /// Arc constrained to the bottom cardinal position.
    ArcAngleBottom,
    /// Arc constrained to the left cardinal position.
    ArcAngleLeft,
    /// Arc constrained to the right cardinal position.
    ArcAngleRight,
    /// Horizontal point alignment.
    HorizontalPoints,
    /// Vertical point alignment.
    VerticalPoints,
    /// Collinearity.
    Collinear,
    /// Circular arcs share a center and radius.
    Coradial,
    /// Point snapped to the sketch grid.
    SnapGrid,
    /// Length snapped to an increment.
    SnapLength,
    /// Angle snapped to an increment.
    SnapAngle,
    /// Geometry converted from an external edge.
    UseEdge,
    /// Ellipse angle fixed at 90 degrees.
    EllipseAngle90,
    /// Ellipse angle fixed at 180 degrees.
    EllipseAngle180,
    /// Ellipse angle fixed at 270 degrees.
    EllipseAngle270,
    /// Ellipse constrained to the top cardinal position.
    EllipseAngleTop,
    /// Ellipse constrained to the bottom cardinal position.
    EllipseAngleBottom,
    /// Ellipse constrained to the left cardinal position.
    EllipseAngleLeft,
    /// Ellipse constrained to the right cardinal position.
    EllipseAngleRight,
    /// Point pierces a referenced entity.
    AtPierce,
    /// Doubled distance display relation.
    DoubleDistance,
    /// Point merge relation.
    MergePoints,
    /// Three-point angular dimension.
    AngleThreePoint,
    /// Arc-length dimension.
    ArcLength,
    /// Entity normal relation.
    Normal,
    /// Point normal relation.
    NormalPoints,
    /// Offset relation between entities in one sketch.
    SketchOffset,
    /// Entity aligned with the X axis.
    AlongX,
    /// Entity aligned with the Y axis.
    AlongY,
    /// Entity aligned with the Z axis.
    AlongZ,
    /// Points aligned with the X axis.
    AlongXPoints,
    /// Points aligned with the Y axis.
    AlongYPoints,
    /// Points aligned with the Z axis.
    AlongZPoints,
    /// Entity parallel to the YZ plane.
    ParallelYz,
    /// Entity parallel to the ZX plane.
    ParallelZx,
    /// Intersection relation.
    Intersection,
    /// Pattern membership relation.
    Patterned,
    /// Isoparametric curve controlled by an external point.
    IsoByPoint,
    /// Common isoparametric relation.
    SameIsoparametric,
    /// Fit-spline relation.
    FitSpline,
    /// Equal-curvature relation.
    EqualCurvature,
    /// Equal-tangent relation.
    EqualTangent,
    /// Tangency to a face.
    TangentFace,
    /// 3D entity aligned with the X axis.
    AlongX3d,
    /// 3D entity aligned with the Y axis.
    AlongY3d,
    /// 3D points aligned with the X axis.
    AlongXPoints3d,
    /// 3D points aligned with the Y axis.
    AlongYPoints3d,
    /// Traction relation.
    Traction,
    /// Belt-traction relation.
    BeltTraction,
    /// Two blocks locked together.
    BlockFixedLock,
    /// Blocks locked normal to one another.
    BlockNormalLock,
    /// Blocks locked for relative rotation.
    BlockRotateLock,
    /// Display-only slot relation.
    FakeSlotConstraint,
    /// Fixed-slot relation.
    FixedSlot,
    /// Slots have equal dimensions.
    SameSlots,
    /// Linear-pattern count relation.
    LinearPatternCount,
    /// Circular-pattern count relation.
    CircularPatternCount,
    /// Radial routing offset.
    RadialOffset,
    /// Planar routing offset.
    PlanarOffset,
    /// Aligned equal curvature between 3D splines.
    EqualCurvature3dAligned,
    /// Virtual-point distance to a flange face.
    FlangeFaceDistance,
    /// Conic rho relation.
    ConicRho,
    /// Third-order continuity relation.
    C3Touch,
    /// Doubled angle display relation.
    DoubleAngle,
    /// Equal arc or spline length.
    SameCurveLength,
}

impl SketchRelationKind {
    /// Decodes relation codes `1..85`.
    pub fn from_native_code(code: u32) -> Option<Self> {
        Some(match code {
            1 => Self::Distance,
            2 => Self::Angle,
            3 => Self::Radius,
            4 => Self::Horizontal,
            5 => Self::Vertical,
            6 => Self::Tangent,
            7 => Self::Parallel,
            8 => Self::Perpendicular,
            9 => Self::Coincident,
            10 => Self::Concentric,
            11 => Self::Symmetric,
            12 => Self::Midpoint,
            13 => Self::AtIntersection,
            14 => Self::Equal,
            15 => Self::Diameter,
            16 => Self::OffsetEdge,
            17 => Self::Fixed,
            18 => Self::ArcAngle90,
            19 => Self::ArcAngle180,
            20 => Self::ArcAngle270,
            21 => Self::ArcAngleTop,
            22 => Self::ArcAngleBottom,
            23 => Self::ArcAngleLeft,
            24 => Self::ArcAngleRight,
            25 => Self::HorizontalPoints,
            26 => Self::VerticalPoints,
            27 => Self::Collinear,
            28 => Self::Coradial,
            29 => Self::SnapGrid,
            30 => Self::SnapLength,
            31 => Self::SnapAngle,
            32 => Self::UseEdge,
            33 => Self::EllipseAngle90,
            34 => Self::EllipseAngle180,
            35 => Self::EllipseAngle270,
            36 => Self::EllipseAngleTop,
            37 => Self::EllipseAngleBottom,
            38 => Self::EllipseAngleLeft,
            39 => Self::EllipseAngleRight,
            40 => Self::AtPierce,
            41 => Self::DoubleDistance,
            42 => Self::MergePoints,
            43 => Self::AngleThreePoint,
            44 => Self::ArcLength,
            45 => Self::Normal,
            46 => Self::NormalPoints,
            47 => Self::SketchOffset,
            48 => Self::AlongX,
            49 => Self::AlongY,
            50 => Self::AlongZ,
            51 => Self::AlongXPoints,
            52 => Self::AlongYPoints,
            53 => Self::AlongZPoints,
            54 => Self::ParallelYz,
            55 => Self::ParallelZx,
            56 => Self::Intersection,
            57 => Self::Patterned,
            58 => Self::IsoByPoint,
            59 => Self::SameIsoparametric,
            60 => Self::FitSpline,
            61 => Self::EqualCurvature,
            62 => Self::EqualTangent,
            63 => Self::TangentFace,
            64 => Self::AlongX3d,
            65 => Self::AlongY3d,
            66 => Self::AlongXPoints3d,
            67 => Self::AlongYPoints3d,
            68 => Self::Traction,
            69 => Self::BeltTraction,
            70 => Self::BlockFixedLock,
            71 => Self::BlockNormalLock,
            72 => Self::BlockRotateLock,
            73 => Self::FakeSlotConstraint,
            74 => Self::FixedSlot,
            75 => Self::SameSlots,
            76 => Self::LinearPatternCount,
            77 => Self::CircularPatternCount,
            78 => Self::RadialOffset,
            79 => Self::PlanarOffset,
            80 => Self::EqualCurvature3dAligned,
            81 => Self::FlangeFaceDistance,
            82 => Self::ConicRho,
            83 => Self::C3Touch,
            84 => Self::DoubleAngle,
            85 => Self::SameCurveLength,
            _ => return None,
        })
    }

    /// Returns the serialized relation code.
    pub fn native_code(self) -> u32 {
        match self {
            Self::Distance => 1,
            Self::Angle => 2,
            Self::Radius => 3,
            Self::Horizontal => 4,
            Self::Vertical => 5,
            Self::Tangent => 6,
            Self::Parallel => 7,
            Self::Perpendicular => 8,
            Self::Coincident => 9,
            Self::Concentric => 10,
            Self::Symmetric => 11,
            Self::Midpoint => 12,
            Self::AtIntersection => 13,
            Self::Equal => 14,
            Self::Diameter => 15,
            Self::OffsetEdge => 16,
            Self::Fixed => 17,
            Self::ArcAngle90 => 18,
            Self::ArcAngle180 => 19,
            Self::ArcAngle270 => 20,
            Self::ArcAngleTop => 21,
            Self::ArcAngleBottom => 22,
            Self::ArcAngleLeft => 23,
            Self::ArcAngleRight => 24,
            Self::HorizontalPoints => 25,
            Self::VerticalPoints => 26,
            Self::Collinear => 27,
            Self::Coradial => 28,
            Self::SnapGrid => 29,
            Self::SnapLength => 30,
            Self::SnapAngle => 31,
            Self::UseEdge => 32,
            Self::EllipseAngle90 => 33,
            Self::EllipseAngle180 => 34,
            Self::EllipseAngle270 => 35,
            Self::EllipseAngleTop => 36,
            Self::EllipseAngleBottom => 37,
            Self::EllipseAngleLeft => 38,
            Self::EllipseAngleRight => 39,
            Self::AtPierce => 40,
            Self::DoubleDistance => 41,
            Self::MergePoints => 42,
            Self::AngleThreePoint => 43,
            Self::ArcLength => 44,
            Self::Normal => 45,
            Self::NormalPoints => 46,
            Self::SketchOffset => 47,
            Self::AlongX => 48,
            Self::AlongY => 49,
            Self::AlongZ => 50,
            Self::AlongXPoints => 51,
            Self::AlongYPoints => 52,
            Self::AlongZPoints => 53,
            Self::ParallelYz => 54,
            Self::ParallelZx => 55,
            Self::Intersection => 56,
            Self::Patterned => 57,
            Self::IsoByPoint => 58,
            Self::SameIsoparametric => 59,
            Self::FitSpline => 60,
            Self::EqualCurvature => 61,
            Self::EqualTangent => 62,
            Self::TangentFace => 63,
            Self::AlongX3d => 64,
            Self::AlongY3d => 65,
            Self::AlongXPoints3d => 66,
            Self::AlongYPoints3d => 67,
            Self::Traction => 68,
            Self::BeltTraction => 69,
            Self::BlockFixedLock => 70,
            Self::BlockNormalLock => 71,
            Self::BlockRotateLock => 72,
            Self::FakeSlotConstraint => 73,
            Self::FixedSlot => 74,
            Self::SameSlots => 75,
            Self::LinearPatternCount => 76,
            Self::CircularPatternCount => 77,
            Self::RadialOffset => 78,
            Self::PlanarOffset => 79,
            Self::EqualCurvature3dAligned => 80,
            Self::FlangeFaceDistance => 81,
            Self::ConicRho => 82,
            Self::C3Touch => 83,
            Self::DoubleAngle => 84,
            Self::SameCurveLength => 85,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SketchInputKind, SketchRelationKind};

    #[test]
    fn marker_layout_disambiguates_geometry_and_relation_codes() {
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(1, true),
            SketchInputKind::LineOrCircle
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(1, false),
            SketchInputKind::Relation(SketchRelationKind::Distance)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(9, false),
            SketchInputKind::Relation(SketchRelationKind::Coincident)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(4, true),
            SketchInputKind::Relation(SketchRelationKind::Horizontal)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(10, true),
            SketchInputKind::Relation(SketchRelationKind::Concentric)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(27, false),
            SketchInputKind::Relation(SketchRelationKind::Collinear)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(28, false),
            SketchInputKind::Relation(SketchRelationKind::Coradial)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(86, false),
            SketchInputKind::Native(86)
        );
        for code in 1..=85 {
            let relation = SketchRelationKind::from_native_code(code).unwrap();
            assert_eq!(relation.native_code(), code);
        }
    }

    #[test]
    fn scalar_bearing_instances_own_dimensional_constraints() {
        for relation in [
            SketchRelationKind::Distance,
            SketchRelationKind::Angle,
            SketchRelationKind::Radius,
            SketchRelationKind::Diameter,
        ] {
            assert!(!SketchInputKind::Relation(relation).owns_constraint());
        }
        assert!(SketchInputKind::Relation(SketchRelationKind::Horizontal).owns_constraint());
        assert!(SketchInputKind::Native(86).owns_constraint());
        assert!(!SketchInputKind::Point.owns_constraint());
    }
}
