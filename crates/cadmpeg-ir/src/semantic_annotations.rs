// SPDX-License-Identifier: Apache-2.0
//! Format-neutral semantic dimensions, notes, symbols, and callouts.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, DeserializeName)]
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
    pub text: Vec<String>,
    /// Ordered references grouped by exact source-property role.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub references: BTreeMap<NonBlankString, Vec<crate::references::ReferenceSelection>>,
    /// The measured value the source stores, in canonical units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_value")]
    pub value: Option<crate::scalar::FiniteReal>,
    /// Ordered page-space reference points, empty or one pair, in millimetres.
    pub reference_anchors: Vec<crate::units::FiniteVector<2>>,
    /// Stored physical text size and family.
    pub text_style: AnnotationTextStyle,
    /// Persisted formatting expression or visible dimension format.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_format"
    )]
    pub format: Option<String>,
    /// The meaning of `format`, where a decoder parses it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_number_format"
    )]
    pub number_format: Option<NumberFormat>,
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
    pub dimension_kind: Option<AnnotationDimensionKind>,
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
    /// Whether a dimension measures the model's references or the view's
    /// projected geometry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_measured_on"
    )]
    pub measured_on: Option<MeasuredOn>,
    /// Whether the dimension is theoretically exact.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_basic"
    )]
    pub basic: Option<bool>,
    /// Whether an angle dimension gives the reflex angle, 2π less the
    /// measured one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reflex"
    )]
    pub reflex: Option<bool>,
    /// Whether a dimension that is not an angle shows the negative of its
    /// measured value.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_negated"
    )]
    pub negated: Option<bool>,
    /// The directions of a distance dimension's lines; `None` lays them out
    /// from the references.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_line_directions"
    )]
    pub line_directions: Option<LineDirections>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<NonBlankString, String>,
    /// Symbol, image, font, or other retained assets.
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
            reference_anchors: Vec::new(),
            text_style: AnnotationTextStyle::default(),
            format: None,
            number_format: None,
            position: None,
            dimension_kind: None,
            text_override: None,
            upper_tolerance: None,
            lower_tolerance: None,
            measured_on: None,
            basic: None,
            reflex: None,
            negated: None,
            line_directions: None,
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

/// The kind of a drawing dimension; an extension type keeps its native
/// selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnnotationDimensionKind {
    /// Linear distance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Distance,
    /// Horizontal distance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    HorizontalDistance,
    /// Vertical distance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    VerticalDistance,
    /// Radius.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Radius,
    /// Diameter.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Diameter,
    /// Angle between two lines.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Angle,
    /// Angle through three points.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    ThreePointAngle,
    /// The area of one face.
    Area {
        /// Whether the area excludes the face's holes; `false` is the area
        /// inside its outer boundary.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_holes_excluded"
        )]
        holes_excluded: Option<bool>,
    },
    /// The extent of the dimension's edges, or of every edge of the view,
    /// along a view axis.
    Extent {
        /// The view axis.
        axis: ViewAxis,
    },
    /// Extension dimension category.
    Other {
        /// Exact native category selector.
        native_type: String,
    },
}

/// An axis of a drawing view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewAxis {
    /// The view's x axis.
    X,
    /// The view's y axis.
    Y,
}

/// What a dimension measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasuredOn {
    /// The model's references.
    Model,
    /// The view's projected geometry, divided by the view's scale.
    View,
}

/// The directions of a distance dimension's lines, counterclockwise in the
/// view plane from the view's x axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LineDirections {
    /// The dimension line's direction.
    pub dimension_line: crate::scalar::Angle,
    /// The extension lines' direction.
    pub extension_lines: crate::scalar::Angle,
}

/// The meaning of a dimension's format text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NumberFormat {
    /// The text before the number.
    pub prefix: String,
    /// The text after the number.
    pub suffix: String,
    /// The digits after the point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_decimals"
    )]
    pub decimals: Option<u8>,
    /// Whether trailing zeros and a trailing point are removed.
    pub trim_zeros: bool,
    /// The increment the number is rounded to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_round_to"
    )]
    pub round_to: Option<crate::scalar::FiniteReal>,
    /// Whether a length's unit symbol follows the number.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit_shown"
    )]
    pub unit_shown: Option<bool>,
}

cadmpeg_core::named_optional_field!(deserialize_holes_excluded, bool, "holes_excluded");
cadmpeg_core::named_optional_field!(deserialize_decimals, u8, "decimals");
cadmpeg_core::named_optional_field!(deserialize_round_to, crate::scalar::FiniteReal, "round_to");
cadmpeg_core::named_optional_field!(deserialize_unit_shown, bool, "unit_shown");
cadmpeg_core::named_optional_field!(deserialize_number_format, NumberFormat, "number_format");
cadmpeg_core::named_optional_field!(deserialize_measured_on, MeasuredOn, "measured_on");
cadmpeg_core::named_optional_field!(deserialize_basic, bool, "basic");
cadmpeg_core::named_optional_field!(deserialize_reflex, bool, "reflex");
cadmpeg_core::named_optional_field!(deserialize_negated, bool, "negated");
cadmpeg_core::named_optional_field!(
    deserialize_line_directions,
    LineDirections,
    "line_directions"
);

cadmpeg_core::named_optional_field!(
    deserialize_dimension_kind,
    AnnotationDimensionKind,
    "dimension_kind"
);
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
    fn physical_text_style_roundtrips_and_names_invalid_height() {
        let wire = serde_json::json!({"id":"test:model:annotation#text", "object":"source", "kind":"text", "runtime_type":"Text", "order":0, "native_ref":"native", "text_style":{"height":5.0,"font_family":"osifont"}, "text": [], "references": {}, "reference_anchors": [], "parameters": {}, "assets": []});
        let annotation: SemanticAnnotation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(annotation).unwrap(), wire);
        for height in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::Value::Null,
        ] {
            let mut rejected = wire.clone();
            rejected["text_style"]["height"] = height;
            assert!(serde_json::from_value::<SemanticAnnotation>(rejected)
                .unwrap_err()
                .to_string()
                .contains("height"));
        }
    }

    #[test]
    fn dimension_wire_admission_names_optional_fields() {
        let wire = serde_json::json!({"id": "synthetic:test:annotation#dimension", "object": "source", "kind": "dimension", "runtime_type": "Dimension", "order": 0, "native_ref": "native",
            "dimension_kind": {"kind": "radius"}, "value": 12.0, "format": "%.2f", "position": [4.0, 5.0, 0.0],
            "text_override": "R12", "upper_tolerance": 0.2, "lower_tolerance": -0.1, "text": [], "references": {}, "reference_anchors": [], "text_style": {}, "parameters": {}, "assets": []});
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

    #[test]
    fn a_unit_dimension_kind_states_its_kind_alone() {
        use super::AnnotationDimensionKind;
        let wire = serde_json::json!({"kind": "distance"});
        assert_eq!(
            serde_json::to_value(AnnotationDimensionKind::Distance).unwrap(),
            wire
        );
        assert_eq!(
            serde_json::from_value::<AnnotationDimensionKind>(wire).unwrap(),
            AnnotationDimensionKind::Distance
        );
        for extra in [serde_json::Value::Null, serde_json::json!({})] {
            assert!(serde_json::from_value::<AnnotationDimensionKind>(
                serde_json::json!({"kind": "distance", "value": extra})
            )
            .is_err());
        }
        let area = serde_json::json!({"kind": "area", "holes_excluded": true});
        assert_eq!(
            serde_json::from_value::<AnnotationDimensionKind>(area.clone()).unwrap(),
            AnnotationDimensionKind::Area {
                holes_excluded: Some(true)
            }
        );
        assert!(serde_json::from_value::<AnnotationDimensionKind>(
            serde_json::json!({"kind": "area", "holes_excluded": null})
        )
        .is_err());
        let extent = serde_json::json!({"kind": "extent", "axis": "y"});
        let read = serde_json::from_value::<AnnotationDimensionKind>(extent.clone()).unwrap();
        assert_eq!(serde_json::to_value(read).unwrap(), extent);
    }

    #[test]
    fn dimension_semantics_round_trip_and_name_their_fields() {
        let wire = serde_json::json!({"id": "synthetic:test:annotation#dimension", "object": "source", "kind": "dimension", "runtime_type": "Dimension", "order": 0, "native_ref": "native",
            "dimension_kind": {"kind": "distance"}, "value": 30.0, "format": "%.2w",
            "number_format": {"prefix": "", "suffix": "", "decimals": 2, "trim_zeros": true, "unit_shown": false},
            "measured_on": "view", "basic": true, "reflex": false, "negated": true,
            "line_directions": {"dimension_line": 0.5, "extension_lines": 2.0},
            "text": [], "references": {}, "reference_anchors": [], "text_style": {}, "parameters": {}, "assets": []});
        let annotation: SemanticAnnotation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(annotation.measured_on, Some(super::MeasuredOn::View));
        assert_eq!(serde_json::to_value(annotation).unwrap(), wire);
        for field in [
            "number_format",
            "measured_on",
            "basic",
            "reflex",
            "negated",
            "line_directions",
        ] {
            let mut rejected = wire.clone();
            rejected[field] = serde_json::Value::Null;
            assert!(serde_json::from_value::<SemanticAnnotation>(rejected)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}

/// Text style admitted with a stored physical font size.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AnnotationTextStyle {
    /// Font em size in millimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_text_height"
    )]
    pub height: Option<crate::scalar::PositiveLength>,
    /// Stored font family name, without a font substitution.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_font_family"
    )]
    pub font_family: Option<String>,
}

impl AnnotationTextStyle {
    /// Whether no physical style is present.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_text_height,
    crate::scalar::PositiveLength,
    "height"
);
cadmpeg_core::named_optional_field!(deserialize_font_family, String, "font_family");

impl SemanticAnnotation {
    /// The most reference anchors this annotation admits: two on a
    /// `Dimension` with a `dimension_kind`, one on any other annotation.
    #[must_use]
    pub fn admitted_reference_anchors(&self) -> usize {
        if self.kind == SemanticAnnotationKind::Dimension && self.dimension_kind.is_some() {
            2
        } else {
            1
        }
    }
}
