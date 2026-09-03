// SPDX-License-Identifier: Apache-2.0
//! Format-neutral drawing sheets, resources, views, and annotations.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
/// Stable identity of one neutral drawing entity.
pub struct DrawingId(#[serde(serialize_with = "crate::schema::serialize_reference_id")] pub String);

/// Semantic role of a drawing entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DrawingKind {
    /// Sheet containing ordered views.
    Page,
    /// Page template or border resource.
    Template,
    /// Model-derived drawing view.
    View,
    /// Projected child view.
    Projection,
    /// Section or detail view.
    Section,
    /// Enlarged detail view.
    Detail,
    /// Measured drawing dimension.
    Dimension,
    /// Text annotation placed on a drawing.
    Annotation,
    /// Balloon or callout annotation.
    Balloon,
    /// Reusable drawing symbol.
    Symbol,
    /// Raster image placed on a drawing.
    Image,
    /// Leader geometry or annotation.
    Leader,
    /// Extension-defined drawing object.
    Other,
}

/// A page, template, view, projection, section, or drawing annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Drawing {
    /// Stable drawing identity.
    pub id: DrawingId,
    /// Application object persisting this drawing entity.
    pub object: String,
    /// Format-neutral semantic role.
    pub kind: DrawingKind,
    /// Exact runtime type for extension-safe classification.
    pub runtime_type: String,
    /// Source order among drawing entities.
    pub order: u32,
    /// Whether the source explicitly displays this drawing entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Ordered relationships grouped by exact source-property role.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub relationships: BTreeMap<String, Vec<crate::references::ReferenceSelection>>,
    /// Page template drawing identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// View origin on its page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 2]>,
    /// Positive view scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Nonzero model projection direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f64; 3]>,
    /// View rotation in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_degrees: Option<f64>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    /// Template, image, symbol, or other retained assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    /// Native drawing record supplying this entity.
    pub native_ref: String,
}
