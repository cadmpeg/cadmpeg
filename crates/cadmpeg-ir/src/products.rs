// SPDX-License-Identifier: Apache-2.0
//! Neutral product structure and occurrence instancing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable product-component identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ComponentId(pub String);

/// Stable placed-occurrence identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct OccurrenceId(pub String);

/// Stable assembly-joint identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct JointId(pub String);

/// Role of a component definition in the product tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    /// Product part or assembly container.
    Part,
    /// Generic ordered object group.
    Group,
    /// Container whose children are link instances.
    LinkGroup,
    /// Reusable leaf object without container semantics.
    Object,
}

/// A reusable product definition or structural container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Component {
    /// Globally unique definition identity.
    pub id: ComponentId,
    /// Structural role.
    pub kind: ComponentKind,
    /// Stable source object name used by product/BOM tooling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    /// User-visible component label, when distinct from the source name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// User-maintained BOM description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// User-maintained part or stock number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<String>,
    /// Additional persisted BOM identity fields by exact property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bom_properties: BTreeMap<String, String>,
    /// Direct containing component, absent for a product root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ComponentId>,
    /// Placement relative to the direct container.
    #[serde(default = "identity_transform")]
    pub local_transform: [[f64; 4]; 4],
    /// Placement composed through all containing components exactly once.
    #[serde(default = "identity_transform")]
    pub resolved_transform: [[f64; 4]; 4],
    /// Direct component definitions in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<ComponentId>,
    /// Direct placed uses in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<OccurrenceId>,
    /// Format-native object supplying this definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

fn identity_transform() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Local or unresolved external prototype of an occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ComponentReference {
    /// Prototype resolves to a definition in this document.
    Local {
        /// Resolved component definition.
        component: ComponentId,
    },
    /// Prototype belongs to another document, loaded or not.
    External {
        /// Persisted external-document reference and unresolved state.
        document: ExternalDocumentReference,
        /// Persisted object identity within that document.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object: Option<String>,
    },
    /// The source intentionally carries no resolvable prototype.
    Unresolved,
}

/// First-class external document reference without implicit loading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalDocumentReference {
    /// File path when the source explicitly stores a file attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Document identity when the source explicitly stores a document attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    /// Deterministic resolution state; decoding never opens external documents.
    pub resolution: ExternalResolution,
}

/// Resolution state of an external product reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResolution {
    /// Target document was not loaded by this decode.
    Unresolved,
    /// Persisted reference was present but empty or structurally unusable.
    MissingReference,
}

/// Copy-on-change ownership behavior of a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "policy", content = "native_policy", rename_all = "snake_case")]
pub enum CopyOnChangePolicy {
    /// Link follows its prototype without making an owned copy.
    Disabled,
    /// Copy is created when a marked prototype property changes.
    Enabled,
    /// Link currently owns a changed copy.
    Owned,
    /// Owned copy continues tracking its original source.
    Tracking,
    /// Future policy retained without reinterpretation.
    Native(String),
}

/// One placed use, including an element of a link array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Occurrence {
    /// Globally unique instance identity.
    pub id: OccurrenceId,
    /// Reusable definition used by this instance.
    pub prototype: ComponentReference,
    /// Direct containing component, absent for a root occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ComponentId>,
    /// Zero-based link-array element index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array_index: Option<u32>,
    /// Placement relative to the direct container.
    pub local_transform: [[f64; 4]; 4],
    /// Linked prototype placement contribution selected by link-transform policy.
    #[serde(default = "identity_transform")]
    pub prototype_transform: [[f64; 4]; 4],
    /// Placement composed through all containers exactly once.
    pub resolved_transform: [[f64; 4]; 4],
    /// Per-axis instance scale.
    pub scale: [f64; 3],
    /// Persisted prototype subelement selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_subelements: Vec<String>,
    /// Per-element visibility override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Explicit application object representing this array element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_component: Option<ComponentId>,
    /// Whether this link claims its prototype in the source tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_child: Option<bool>,
    /// Copy-on-change ownership policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change: Option<CopyOnChangePolicy>,
    /// Original component tracked by copy-on-change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change_source: Option<ComponentId>,
    /// Internal component holding owned copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change_group: Option<ComponentId>,
    /// Whether the tracked source was persisted as changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_on_change_touched: Option<bool>,
    /// Whether the prototype placement participates in evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_transform: Option<bool>,
    /// Format-native object supplying this instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Neutral family of an assembly joint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "native_kind", rename_all = "snake_case")]
pub enum JointKind {
    /// Rigid connection with no relative degrees of freedom.
    Fixed,
    /// Rotation about one axis.
    Revolute,
    /// Translation along one axis.
    Slider,
    /// Coupled rotation and translation on one axis.
    Cylindrical,
    /// Rotation about a common point.
    Ball,
    /// Maintains a scalar separation.
    Distance,
    /// Maintains parallel connector directions.
    Parallel,
    /// Maintains perpendicular connector directions.
    Perpendicular,
    /// Maintains an angular separation.
    Angle,
    /// Couples rack translation to pinion rotation.
    RackPinion,
    /// Couples translation and rotation by screw pitch.
    Screw,
    /// Couples two gear rotations.
    Gears,
    /// Couples two pulley rotations through a belt.
    Belt,
    /// Persisted grounding of a component.
    Grounded,
    /// Future application-defined family retained without relabeling.
    Native(String),
}

/// One connector operand and its selected native subelements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JointOperand {
    /// Local component when the object resolves within this document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentId>,
    /// External document token when resolution is intentionally deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_document: Option<ExternalDocumentReference>,
    /// Exact referenced application object identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// Ordered persistent object/element paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subelements: Vec<String>,
}

/// Optional enabled interval for a joint degree of freedom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JointLimits {
    /// Lower bound when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// Upper bound when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

/// Neutral assembly constraint between connector frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssemblyJoint {
    /// Globally unique joint identity.
    pub id: JointId,
    /// Joint kinematic family.
    pub kind: JointKind,
    /// Ordered connector or grounded-object operands.
    pub operands: Vec<JointOperand>,
    /// Connector-local frames in operand order.
    pub frames: Vec<[[f64; 4]; 4]>,
    /// Connector attachment offsets in operand order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset_frames: Vec<[[f64; 4]; 4]>,
    /// Whether solving this joint is suppressed.
    pub suppressed: bool,
    /// Per-connector detach flags.
    pub detached: [bool; 2],
    /// Angular offset in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Primary linear offset in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Secondary linear offset in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<f64>,
    /// Enabled angular interval in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angular_limits: Option<JointLimits>,
    /// Enabled linear interval in document length units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_limits: Option<JointLimits>,
    /// Exact persisted scalar state, including future controls.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Format-native joint record supplying this constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}
