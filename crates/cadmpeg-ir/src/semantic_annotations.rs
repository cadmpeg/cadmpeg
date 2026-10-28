// SPDX-License-Identifier: Apache-2.0
//! Format-neutral semantic dimensions, notes, symbols, and callouts.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

crate::ids::id_type!(
    /// Stable semantic-annotation identity.
    SemanticAnnotationId, compose
);

/// Semantic role of an annotation independent of its drawing presentation.
///
/// GD&T frames and datum features are modelled in [`crate::pmi`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SemanticAnnotationKind {
    /// Measured linear, angular, radial, or other dimension.
    Dimension,
    /// Free or model-associated text note.
    Text,
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
#[serde(deny_unknown_fields)]
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
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub references: BTreeMap<NonBlankString, Vec<crate::references::ReferenceSelection>>,
    /// Persisted numeric measurement, when explicitly carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_value")]
    pub value: Option<crate::scalar::FiniteReal>,
    /// Persisted formatting expression or visible dimension format.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_format"
    )]
    pub format: Option<String>,
    /// Persisted model- or page-space annotation position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_position")]
    pub position: Option<crate::units::FiniteVector<3>>,
    /// Dimension category; extension types retain their native selector.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_dimension_kind"
    )]
    pub dimension_kind: Option<DimensionKind>,
    /// Explicit replacement text when arbitrary dimension text is enabled.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_text_override"
    )]
    pub text_override: Option<String>,
    /// Upper signed tolerance in the measurement unit; zero means unset.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_upper_tolerance"
    )]
    pub upper_tolerance: Option<crate::scalar::FiniteReal>,
    /// Lower signed tolerance in the measurement unit; zero means unset.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lower_tolerance"
    )]
    pub lower_tolerance: Option<crate::scalar::FiniteReal>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<NonBlankString, String>,
    /// Symbol, image, font, or other retained assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    /// Native semantic annotation record supplying this entity.
    pub native_ref: String,
}

impl SemanticAnnotation {
    /// Construct a record with absent optional content and empty collections.
    #[must_use]
    pub fn new(
        id: SemanticAnnotationId,
        object: String,
        kind: SemanticAnnotationKind,
        runtime_type: String,
        order: u32,
        native_ref: String,
    ) -> Self {
        Self {
            id,
            object,
            kind,
            runtime_type,
            order,
            native_ref,
            text: Vec::new(),
            references: BTreeMap::new(),
            value: None,
            format: None,
            position: None,
            dimension_kind: None,
            text_override: None,
            upper_tolerance: None,
            lower_tolerance: None,
            parameters: BTreeMap::new(),
            assets: Vec::new(),
        }
    }
}

cadmpeg_core::named_optional_field!(deserialize_value, crate::scalar::FiniteReal, "value");

cadmpeg_core::named_optional_field!(
    deserialize_position,
    crate::units::FiniteVector<3>,
    "position"
);

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_format, String, "format");

mod identity_rewrite;

/// Neutral dimension category. Linear values use millimetres; angular values use degrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionKind {
    /// Linear distance.
    Distance {},
    /// Horizontal distance.
    HorizontalDistance {},
    /// Vertical distance.
    VerticalDistance {},
    /// Radius.
    Radius {},
    /// Diameter.
    Diameter {},
    /// Angle between two lines.
    Angle {},
    /// Angle through three points.
    ThreePointAngle {},
    /// Extension dimension category.
    Other {
        /// Exact native category selector.
        native_type: String,
    },
}

cadmpeg_core::named_optional_field!(deserialize_dimension_kind, DimensionKind, "dimension_kind");
cadmpeg_core::named_optional_field!(deserialize_text_override, String, "text_override");
cadmpeg_core::named_optional_field!(
    deserialize_upper_tolerance,
    crate::scalar::FiniteReal,
    "upper_tolerance"
);
cadmpeg_core::named_optional_field!(
    deserialize_lower_tolerance,
    crate::scalar::FiniteReal,
    "lower_tolerance"
);

#[cfg(test)]
mod tests {
    use super::SemanticAnnotation;

    #[test]
    fn dimension_wire_admission_names_optional_fields() {
        let wire = serde_json::json!({"id": "synthetic:test:annotation#dimension", "object": "source", "kind": "dimension", "runtime_type": "Dimension", "order": 0, "native_ref": "native",
            "dimension_kind": {"kind": "radius"}, "value": 12.0, "format": "%.2f", "position": [4.0, 5.0, 0.0],
            "text_override": "R12", "upper_tolerance": 0.2, "lower_tolerance": -0.1});
        let annotation: SemanticAnnotation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(annotation).unwrap(), wire);
        for field in [
            "dimension_kind",
            "text_override",
            "upper_tolerance",
            "lower_tolerance",
        ] {
            let mut rejected = wire.clone();
            rejected[field] = serde_json::json!([]);
            assert!(serde_json::from_value::<SemanticAnnotation>(rejected)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let mut unknown = wire.clone();
        unknown["dimension_kind"]["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<SemanticAnnotation>(unknown)
            .unwrap_err()
            .to_string()
            .contains("dimension_kind"));
        let mut other = wire;
        other["dimension_kind"] = serde_json::json!({"kind": "other", "native_type": "extension"});
        let annotation: SemanticAnnotation = serde_json::from_value(other.clone()).unwrap();
        assert_eq!(serde_json::to_value(annotation).unwrap(), other);
    }
}
