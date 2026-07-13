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
    /// Typed sketch-entity markers located within `native_payload`.
    #[serde(default)]
    pub sketch_entities: Vec<SketchInputEntity>,
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
    /// Sketch-entity kind this marker identifies.
    pub kind: SketchInputKind,
}

/// Kind of sketch entity referenced by a native feature-input marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchInputKind {
    /// A sketch point.
    Point,
    /// A general sketch curve.
    Curve,
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
            1 => Self::Curve,
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
            Self::Curve => 1,
            Self::Arc => 2,
            Self::ConstrainedPoint => 3,
            Self::Native(value) => value,
        }
    }
}
