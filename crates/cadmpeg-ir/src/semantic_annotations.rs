// SPDX-License-Identifier: Apache-2.0
//! Format-neutral semantic dimensions, notes, symbols, and callouts.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable semantic-annotation identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SemanticAnnotationId(
    #[serde(serialize_with = "crate::schema::serialize_reference_id")] pub String,
);

/// Semantic role of an annotation independent of its drawing presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SemanticAnnotationKind {
    /// Measured linear, angular, radial, or other dimension.
    Dimension,
    /// Free or model-associated text note.
    Text,
    /// Geometric tolerance frame.
    GeometricTolerance,
    /// Datum feature or datum target.
    Datum,
    /// Numbered or named callout balloon.
    Balloon,
    /// Leader associated with a semantic callout.
    Leader,
    /// Reusable semantic symbol.
    Symbol,
    /// Extension-defined semantic annotation.
    Other,
}

/// Semantic content of a persisted annotation, separate from drawing appearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SemanticAnnotation {
    /// Stable semantic identity.
    pub id: SemanticAnnotationId,
    /// Application object persisting this annotation.
    pub object: String,
    /// Format-neutral semantic role.
    pub kind: SemanticAnnotationKind,
    /// Exact source runtime type.
    pub runtime_type: String,
    /// Source order among semantic annotations.
    pub order: u32,
    /// Ordered visible text fragments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<String>,
    /// Ordered references grouped by exact source-property role.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, Vec<crate::references::ReferenceSelection>>,
    /// Persisted numeric measurement, when explicitly carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Persisted formatting expression or visible dimension format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Persisted model- or page-space annotation position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 3]>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    /// Symbol, image, font, or other retained assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    /// Native semantic annotation record supplying this entity.
    pub native_ref: String,
}
