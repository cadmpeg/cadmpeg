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
        /// Persisted document token or path.
        document: String,
        /// Persisted object identity within that document.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object: Option<String>,
    },
    /// The source intentionally carries no resolvable prototype.
    Unresolved,
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
    /// Placement composed through all containers exactly once.
    pub resolved_transform: [[f64; 4]; 4],
    /// Per-axis instance scale.
    pub scale: [f64; 3],
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
    pub external_document: Option<String>,
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
