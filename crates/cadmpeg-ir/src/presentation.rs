// SPDX-License-Identifier: Apache-2.0
//! Neutral persisted document and view presentation state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable presentation-document identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct PresentationId(pub String);

/// Persisted camera pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraState {
    /// Camera position in document coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 3]>,
    /// Persisted orientation quaternion in source component order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<[f64; 4]>,
    /// Other camera fields retained by exact source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// Ordered non-provider GUI state such as clipping or section state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationState {
    /// Persisted state element name.
    pub kind: String,
    /// Source order among document GUI state elements.
    pub order: u32,
    /// Exact root attributes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    /// Referenced display assets as global native entry ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
}

/// Document-wide persisted GUI state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationDocument {
    /// Globally unique presentation identity.
    pub id: PresentationId,
    /// Persisted GUI schema version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// Active view name or identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_view: Option<String>,
    /// Persisted active camera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraState>,
    /// Ordered document-level GUI states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<PresentationState>,
    /// Native GUI document record supplying this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Presentation state owned by one persisted view provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewPresentation {
    /// Globally unique view-provider identity.
    pub id: PresentationId,
    /// Owning application object identity, if resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// Source order in the provider table.
    pub order: u32,
    /// Persisted tree expansion state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Persisted object visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Display mode name or numeric code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_mode: Option<String>,
    /// Selection rendering mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_style: Option<String>,
    /// Line width in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_width: Option<f64>,
    /// Point size in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_size: Option<f64>,
    /// Remaining view properties by exact source property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Native view-provider record supplying this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}
