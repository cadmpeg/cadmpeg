// SPDX-License-Identifier: Apache-2.0
//! Neutral product structure and occurrence instancing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable product-component identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ComponentId(pub String);

/// Stable placed-occurrence identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct OccurrenceId(pub String);

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
