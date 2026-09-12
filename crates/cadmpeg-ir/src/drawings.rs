// SPDX-License-Identifier: Apache-2.0
//! Format-neutral drawing sheets, resources, views, and annotations.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

crate::ids::id_type!(
    /// Stable identity of one neutral drawing entity.
    DrawingId
);

/// Semantic role of a drawing entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    pub template: Option<DrawingId>,
    /// View origin on its page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_position")]
    pub position: Option<crate::units::FiniteVector<2>>,
    /// Positive view scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_scale")]
    pub scale: Option<crate::units::PositiveScalar>,
    /// Nonzero model projection direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_direction")]
    pub direction: Option<crate::units::NonzeroVector<3>>,
    /// View rotation in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_rotation_degrees")]
    pub rotation_degrees: Option<crate::units::FiniteScalar>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    /// Template, image, symbol, or other retained assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    /// Native drawing record supplying this entity.
    pub native_ref: String,
}

crate::units::named_field!(
    deserialize_position,
    Option<crate::units::FiniteVector<2>>,
    "position"
);

crate::units::named_field!(
    deserialize_scale,
    Option<crate::units::PositiveScalar>,
    "scale"
);

crate::units::named_field!(
    deserialize_direction,
    Option<crate::units::NonzeroVector<3>>,
    "direction"
);

crate::units::named_field!(
    deserialize_rotation_degrees,
    Option<crate::units::FiniteScalar>,
    "rotation_degrees"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_reference_preserves_string_wire_and_rejects_invalid_ids() {
        let value = serde_json::json!({"id": "synthetic:test:drawing#page", "object": "source", "kind": "page", "runtime_type": "Page", "order": 0, "template": "synthetic:test:drawing#template", "native_ref": "native"});
        let drawing: Drawing = serde_json::from_value(value.clone()).expect("valid drawing");
        assert_eq!(
            drawing.template.as_ref().map(DrawingId::as_str),
            Some("synthetic:test:drawing#template")
        );
        assert_eq!(
            serde_json::to_value(drawing).expect("serialize drawing"),
            value
        );
        for invalid in ["", "bad id"] {
            let mut invalid_value = value.clone();
            invalid_value["template"] = serde_json::json!(invalid);
            assert!(serde_json::from_value::<Drawing>(invalid_value).is_err());
        }
    }
    #[test]
    fn numeric_admission_names_fields_and_preserves_valid_values() {
        let wire = serde_json::json!({"id": "synthetic:test:drawing#view", "object": "source", "kind": "view", "runtime_type": "View", "order": 0, "native_ref": "native", "position": [-2.0, 0.0], "scale": 0.5, "direction": [0.0, 0.0, 2.0], "rotation_degrees": -90.0});
        let drawing: Drawing = serde_json::from_value(wire.clone()).expect("valid drawing");
        assert_eq!(serde_json::to_value(drawing).expect("serialize"), wire);
        for (field, invalid) in [
            ("scale", serde_json::json!(0)),
            ("direction", serde_json::json!([0, 0, 0])),
            ("position", serde_json::json!([0, null])),
        ] {
            let mut rejected = wire.clone();
            rejected[field] = invalid;
            let error =
                serde_json::from_value::<Drawing>(rejected).expect_err("invalid numeric field");
            assert!(error.to_string().contains(field));
        }
    }
}
