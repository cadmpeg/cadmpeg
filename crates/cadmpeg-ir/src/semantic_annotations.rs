// SPDX-License-Identifier: Apache-2.0
//! Format-neutral semantic dimensions, notes, symbols, and callouts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable semantic-annotation identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct SemanticAnnotationId(pub String);

/// Semantic role of an annotation independent of its drawing presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// One model, drawing, or external reference used by an annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SemanticAnnotationTarget {
    /// Resolved local model, drawing, or application-object identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// External document token, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_document: Option<String>,
    /// Stable source object token within an external document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_object: Option<String>,
    /// Whether the persisted reference is an explicit null target.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_null: bool,
    /// Ordered model subelement selectors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subelements: Vec<String>,
}

/// Semantic content of a persisted annotation, separate from drawing appearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    pub references: BTreeMap<String, Vec<SemanticAnnotationTarget>>,
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
