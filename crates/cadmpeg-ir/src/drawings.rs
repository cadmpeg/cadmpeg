// SPDX-License-Identifier: Apache-2.0
//! Format-neutral drawing sheets, resources, views, and annotations.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

crate::ids::id_type!(
    /// Stable identity of one neutral drawing entity.
    DrawingId, compose
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Ordered relationships grouped by exact source-property role.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub relationships: BTreeMap<NonBlankString, Vec<crate::references::ReferenceSelection>>,
    /// Page template drawing identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_template"
    )]
    pub template: Option<DrawingId>,
    /// View origin on its page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_position")]
    pub position: Option<crate::units::FiniteVector<2>>,
    /// Positive view scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_scale")]
    pub scale: Option<crate::scalar::PositiveReal>,
    /// Nonzero model projection direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_direction")]
    pub direction: Option<crate::units::NonzeroVector<3>>,
    /// View rotation in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_rotation_degrees")]
    pub rotation_degrees: Option<crate::scalar::FiniteReal>,
    /// Sheet dimensions in millimetres and ordered title-block fields.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sheet"
    )]
    pub sheet: Option<DrawingSheet>,
    /// Curves in source order. Origin is the view origin; X points right and Y points up.
    /// Coordinates are millimetres before view scale and rotation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<DrawingCurve>,
    /// Page-space template primitives and positioned text; template owners only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_template_geometry"
    )]
    pub template_geometry: Option<TemplateGeometry>,
    /// Explicit horizontal model axis; view owners only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_x_direction"
    )]
    pub x_direction: Option<crate::units::NonzeroVector<3>>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<NonBlankString, String>,
    /// Template, image, symbol, or other retained assets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    /// Native drawing record supplying this entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

impl Drawing {
    /// Construct a record with absent optional content and empty collections.
    #[must_use]
    pub fn new(
        id: DrawingId,
        object: String,
        kind: DrawingKind,
        runtime_type: String,
        order: u32,
        native_ref: Option<String>,
    ) -> Self {
        Self {
            id,
            object,
            kind,
            runtime_type,
            order,
            native_ref,
            visible: None,
            relationships: BTreeMap::new(),
            template: None,
            position: None,
            scale: None,
            direction: None,
            rotation_degrees: None,
            template_geometry: None,
            x_direction: None,
            sheet: None,
            curves: Vec::new(),
            parameters: BTreeMap::new(),
            assets: Vec::new(),
        }
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_position,
    crate::units::FiniteVector<2>,
    "position"
);

cadmpeg_core::named_optional_field!(deserialize_scale, crate::scalar::PositiveReal, "scale");

cadmpeg_core::named_optional_field!(
    deserialize_direction,
    crate::units::NonzeroVector<3>,
    "direction"
);

cadmpeg_core::named_optional_field!(
    deserialize_rotation_degrees,
    crate::scalar::FiniteReal,
    "rotation_degrees"
);

#[cfg(test)]
mod tests {
    use super::{Drawing, DrawingId};

    #[test]
    fn native_link_preserves_absence_and_string_and_refuses_null_or_nonstrings() {
        let mut value = serde_json::json!({
            "id": "synthetic:test:drawing#page", "object": "source",
            "kind": "page", "runtime_type": "Page", "order": 0
        });
        let drawing: Drawing =
            serde_json::from_value(value.clone()).expect("no native counterpart");
        assert_eq!(drawing.native_ref, None);
        assert_eq!(serde_json::to_value(drawing).unwrap(), value);

        value["native_ref"] = serde_json::json!("test:source:drawing#one");
        let drawing: Drawing = serde_json::from_value(value.clone()).expect("native counterpart");
        assert_eq!(
            drawing.native_ref.as_deref(),
            Some("test:source:drawing#one")
        );
        assert_eq!(serde_json::to_value(drawing).unwrap(), value);

        for invalid in [
            serde_json::Value::Null,
            serde_json::json!(false),
            serde_json::json!(7),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            value["native_ref"] = invalid;
            assert!(serde_json::from_value::<Drawing>(value.clone()).is_err());
        }
    }

    #[test]
    fn sheet_and_curves_wire_admission() {
        let wire = serde_json::json!({"id": "synthetic:test:drawing#view", "object": "source", "kind": "view", "runtime_type": "View", "order": 0, "native_ref": "native",
        "sheet": {"width_mm": 210.0, "height_mm": 297.0, "fields": [{"name": "Title", "value": "Test"}]},
        "curves": [
            {"geometry": {"kind": "line", "start": [0.0, 1.0], "end": [2.0, 3.0]}, "visibility": "hidden", "source": "projected_model_edge"},
            {"geometry": {"kind": "circle", "center": [0.0, 0.0], "radius": 2.0}, "visibility": "visible", "source": "cosmetic"},
            {"geometry": {"kind": "arc", "center": [0.0, 0.0], "radius": 2.0, "start": [2.0, 0.0], "middle": [0.0, 2.0], "end": [-2.0, 0.0]}, "visibility": "visible", "source": "center_line"}
        ]});
        let drawing: Drawing = serde_json::from_value(wire.clone()).unwrap();
        assert!(drawing.sheet.as_ref().unwrap().is_valid());
        assert!(drawing.curves.iter().all(super::DrawingCurve::is_valid));
        assert_eq!(serde_json::to_value(drawing).unwrap(), wire);
        let mut rejected = wire.clone();
        rejected["sheet"]["width_mm"] = serde_json::json!(0);
        assert!(serde_json::from_value::<Drawing>(rejected)
            .unwrap_err()
            .to_string()
            .contains("sheet"));
        let mut rejected = wire.clone();
        rejected["curves"][1]["geometry"]["radius"] = serde_json::json!(-1);
        assert!(serde_json::from_value::<Drawing>(rejected).is_err());
        let mut rejected = wire.clone();
        rejected["curves"][0]["geometry"]["start"] = serde_json::json!([0, null]);
        assert!(serde_json::from_value::<Drawing>(rejected).is_err());
        let mut rejected = wire;
        rejected["curves"][0]
            .as_object_mut()
            .unwrap()
            .remove("visibility");
        assert!(serde_json::from_value::<Drawing>(rejected)
            .unwrap_err()
            .to_string()
            .contains("visibility"));
    }

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

    #[test]
    fn a_free_form_map_refuses_a_blank_key() {
        let wire = serde_json::json!({
            "id": "synthetic:test:drawing#view",
            "object": "source",
            "kind": "view",
            "runtime_type": "View",
            "order": 0,
            "native_ref": "native",
            "parameters": {"   ": "value"},
        });
        let error = serde_json::from_value::<Drawing>(wire).expect_err("blank parameter role");
        assert!(error.to_string().contains("must not be blank"), "{error}");

        let relationships = serde_json::json!({
            "id": "synthetic:test:drawing#view",
            "object": "source",
            "kind": "view",
            "runtime_type": "View",
            "order": 0,
            "native_ref": "native",
            "relationships": {"": []},
        });
        let error =
            serde_json::from_value::<Drawing>(relationships).expect_err("blank relationship role");
        assert!(error.to_string().contains("must not be blank"), "{error}");

        let named = serde_json::json!({
            "id": "synthetic:test:drawing#view",
            "object": "source",
            "kind": "view",
            "runtime_type": "View",
            "order": 0,
            "native_ref": "native",
            "parameters": {" scale ": "value"},
        });
        let drawing: Drawing =
            serde_json::from_value(named).expect("a named role is kept verbatim");
        assert!(drawing.parameters.contains_key(" scale "));
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
cadmpeg_core::named_optional_field!(deserialize_template, DrawingId, "template");

mod identity_rewrite;

/// Sheet size and editable title-block content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DrawingSheet {
    /// Positive sheet width in millimetres.
    pub width_mm: crate::scalar::PositiveReal,
    /// Positive sheet height in millimetres.
    pub height_mm: crate::scalar::PositiveReal,
    /// Ordered name/value pairs; names are unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<TitleBlockField>,
}

/// One editable title-block field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TitleBlockField {
    /// Field name.
    pub name: NonBlankString,
    /// Field value, including an empty value.
    pub value: String,
}

/// Hidden-line classification, independent of presentation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveVisibility {
    /// Visible geometry.
    Visible,
    /// Geometry behind a model face.
    Hidden,
}

/// Curve origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveSource {
    /// Projected model edge.
    ProjectedModelEdge,
    /// Added drawing geometry.
    Cosmetic,
    /// Center line.
    CenterLine,
}

/// Ordered view curve with mandatory visibility and source classes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DrawingCurve {
    /// Geometry in the owning view frame.
    pub geometry: DrawingCurveGeometry,
    /// Hidden-line class.
    pub visibility: CurveVisibility,
    /// Explicit presentation visibility, separate from hidden-line class.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_displayed"
    )]
    pub displayed: Option<bool>,
    /// Geometry source class.
    pub source: CurveSource,
}

/// Analytic two-dimensional geometry in the frame specified by its owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DrawingCurveGeometry {
    /// Finite line segment.
    Line {
        /// Start point.
        start: crate::units::FiniteVector<2>,
        /// End point.
        end: crate::units::FiniteVector<2>,
    },
    /// Complete circle.
    Circle {
        /// Center point.
        center: crate::units::FiniteVector<2>,
        /// Positive radius in millimetres.
        radius: crate::scalar::PositiveReal,
    },
    /// Circular arc from start through middle to end, with distinct points.
    Arc {
        /// Center point.
        center: crate::units::FiniteVector<2>,
        /// Positive radius in millimetres.
        radius: crate::scalar::PositiveReal,
        /// Start point.
        start: crate::units::FiniteVector<2>,
        /// Interior point selecting the sweep.
        middle: crate::units::FiniteVector<2>,
        /// End point.
        end: crate::units::FiniteVector<2>,
    },
}

impl DrawingSheet {
    /// Check dimensions and unique field names.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let mut names = std::collections::BTreeSet::new();
        self.width_mm.get().is_finite()
            && self.width_mm.get() > 0.0
            && self.height_mm.get().is_finite()
            && self.height_mm.get() > 0.0
            && self
                .fields
                .iter()
                .all(|field| names.insert(field.name.as_str()))
    }
}

const CURVE_RADIAL_RELATIVE_TOLERANCE: f64 = 1e-9;

impl DrawingCurve {
    /// Check numeric and arc-form invariants.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.geometry.is_valid()
    }
}

impl DrawingCurveGeometry {
    /// Check finite geometry and circular arc form in the caller's frame.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let finite =
            |point: crate::units::FiniteVector<2>| point.get().iter().all(|v| v.is_finite());
        let positive =
            |radius: crate::scalar::PositiveReal| radius.get().is_finite() && radius.get() > 0.0;
        match *self {
            DrawingCurveGeometry::Line { start, end } => finite(start) && finite(end),
            DrawingCurveGeometry::Circle { center, radius } => finite(center) && positive(radius),
            DrawingCurveGeometry::Arc {
                center,
                radius,
                start,
                middle,
                end,
            } => {
                finite(center)
                    && positive(radius)
                    && finite(start)
                    && finite(middle)
                    && finite(end)
                    && start != middle
                    && middle != end
                    && start != end
                    && [start, middle, end].iter().all(|point| {
                        let [x, y] = point.get();
                        let [cx, cy] = center.get();
                        ((x - cx).hypot(y - cy) - radius.get()).abs()
                            <= CURVE_RADIAL_RELATIVE_TOLERANCE * radius.get()
                    })
            }
        }
    }
}

cadmpeg_core::named_optional_field!(deserialize_sheet, DrawingSheet, "sheet");

cadmpeg_core::named_optional_field!(deserialize_displayed, bool, "displayed");

/// Ordered template geometry and baseline-positioned text in page millimetres.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TemplateGeometry {
    /// Curves in source order.
    pub curves: Vec<DrawingCurveGeometry>,
    /// Text in source order.
    pub texts: Vec<TemplateText>,
}

impl TemplateGeometry {
    /// Shared admission gate for curve populations.
    pub fn is_valid(&self) -> bool {
        self.curves.iter().all(DrawingCurveGeometry::is_valid)
    }
}

/// Plain template text with resolved editable content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TemplateText {
    /// Resolved Unicode string.
    pub text: String,
    /// Left baseline position in page millimetres.
    pub position: crate::units::FiniteVector<2>,
    /// Font em size in millimetres.
    pub height: crate::scalar::PositiveLength,
}

cadmpeg_core::named_optional_field!(
    deserialize_template_geometry,
    TemplateGeometry,
    "template_geometry"
);
cadmpeg_core::named_optional_field!(
    deserialize_x_direction,
    crate::units::NonzeroVector<3>,
    "x_direction"
);

impl Drawing {
    /// Check ownership and geometry of physical template and view fields.
    #[must_use]
    pub fn physical_content_is_valid(&self) -> bool {
        (self.template_geometry.is_none() || self.kind == DrawingKind::Template)
            && self
                .template_geometry
                .as_ref()
                .is_none_or(TemplateGeometry::is_valid)
            && (self.x_direction.is_none()
                || matches!(
                    self.kind,
                    DrawingKind::View
                        | DrawingKind::Projection
                        | DrawingKind::Section
                        | DrawingKind::Detail
                ))
    }
}
