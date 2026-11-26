// SPDX-License-Identifier: Apache-2.0
//! Format-neutral drawing sheets, resources, views, and annotations.

use crate::math::portable::Portable;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

crate::ids::id_type!(
    /// Stable identity of one neutral drawing entity.
    DrawingId, compose
);

/// Semantic role of a drawing entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, DeserializeName)]
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
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub relationships: BTreeMap<NonBlankString, Vec<crate::references::ReferenceSelection>>,
    /// Page template drawing identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_template"
    )]
    pub template: Option<DrawingId>,
    /// The drawing records a page shows or a view collection holds, in
    /// source order, or in id order where the source's list is a set.
    pub views: Vec<DrawingId>,
    /// The view a section or detail view is cut or magnified from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_base_view"
    )]
    pub base_view: Option<DrawingId>,
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
    /// Counterclockwise view rotation on the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_rotation")]
    pub rotation: Option<crate::scalar::Angle>,
    /// Sheet dimensions in millimetres and ordered title-block fields.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sheet"
    )]
    pub sheet: Option<DrawingSheet>,
    /// Curves in source order. Origin is the view origin; X points right and Y points up.
    /// Coordinates are millimetres before view scale and rotation.
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
    /// Where a view's scale comes from.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scale_from"
    )]
    pub scale_from: Option<ScaleFrom>,
    /// A sheet's or a projection group's projection angle; a group with
    /// `None` takes its sheet's.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_projection"
    )]
    pub projection: Option<ProjectionAngle>,
    /// The focus distance of a perspective view.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_perspective"
    )]
    pub perspective: Option<crate::scalar::PositiveLength>,
    /// The edge classes a view's hidden-line projection draws besides its
    /// visible hard edges.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_classes"
    )]
    pub edge_classes: Option<EdgeClasses>,
    /// How a view's hidden-line projection computes its curves.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hidden_line"
    )]
    pub hidden_line: Option<HiddenLineMethod>,
    /// Remaining typed or exactly framed parameters by source name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<NonBlankString, String>,
    /// Template, image, symbol, or other retained assets.
    pub assets: Vec<String>,
    /// Native drawing record supplying this entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

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
            views: Vec::new(),
            base_view: None,
            position: None,
            scale: None,
            direction: None,
            rotation: None,
            template_geometry: None,
            x_direction: None,
            scale_from: None,
            projection: None,
            perspective: None,
            edge_classes: None,
            hidden_line: None,
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

cadmpeg_core::named_optional_field!(deserialize_rotation, crate::scalar::Angle, "rotation");
cadmpeg_core::named_optional_field!(deserialize_scale_from, ScaleFrom, "scale_from");
cadmpeg_core::named_optional_field!(deserialize_projection, ProjectionAngle, "projection");
cadmpeg_core::named_optional_field!(
    deserialize_perspective,
    crate::scalar::PositiveLength,
    "perspective"
);
cadmpeg_core::named_optional_field!(deserialize_edge_classes, EdgeClasses, "edge_classes");
cadmpeg_core::named_optional_field!(deserialize_hidden_line, HiddenLineMethod, "hidden_line");
cadmpeg_core::named_optional_field!(deserialize_iso_count, u32, "iso_count");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
/// Where a view's scale comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScaleFrom {
    /// The sheet's scale.
    Sheet,
    /// A scale chosen to fit the sheet.
    Fit,
    /// The view's own `scale`.
    Own,
}

/// A projection angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionAngle {
    /// First-angle projection.
    First,
    /// Third-angle projection.
    Third,
}

/// The edge classes a hidden-line projection draws besides its visible hard
/// edges, which every projection draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EdgeClasses {
    /// Each class drawn; a class the set does not hold is not drawn.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<EdgeClass>"))]
    pub drawn: BTreeSet<EdgeClass>,
    /// The number of iso-parameter lines.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_iso_count"
    )]
    pub iso_count: Option<u32>,
}

/// An edge class a hidden-line projection can draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EdgeClass {
    /// Visible edges between tangent faces.
    VisibleSmooth,
    /// Visible seam edges of closed faces.
    VisibleSeam,
    /// Visible iso-parameter lines.
    VisibleIso,
    /// Hidden edges between faces that meet at an angle.
    HiddenHard,
    /// Hidden edges between tangent faces.
    HiddenSmooth,
    /// Hidden seam edges of closed faces.
    HiddenSeam,
    /// Hidden iso-parameter lines.
    HiddenIso,
}

/// How a hidden-line projection computes its curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HiddenLineMethod {
    /// From the exact geometry.
    Exact,
    /// From the shape's triangulation: every curve is a chain of line
    /// segments.
    Polygonal,
}

#[cfg(test)]
mod tests {
    use super::{Drawing, DrawingId};
    use cadmpeg_core::serde_erase::json;

    #[test]
    fn template_text_round_trips_without_a_font_height() {
        let wire = cadmpeg_core::json!({"text": "label", "position": [10.0, 9.0]});
        let text: super::TemplateText = json::from_value(wire.clone()).unwrap();
        assert!(text.height.is_none());
        assert_eq!(json::to_value(text).unwrap(), wire);
        for invalid in [
            cadmpeg_core::json!(null),
            cadmpeg_core::json!(0),
            cadmpeg_core::json!(-1),
        ] {
            let mut rejected = wire.clone();
            rejected["height"] = invalid;
            assert!(json::from_value::<super::TemplateText>(rejected).is_err());
        }
    }

    #[test]
    fn sheet_and_curves_wire_admission() {
        let wire = cadmpeg_core::json!({"id": "synthetic:test:drawing#view", "object": "source", "kind": "view", "runtime_type": "View", "order": 0, "native_ref": "native",
        "sheet": {"width_mm": 210.0, "height_mm": 297.0, "fields": [{"name": "Title", "value": "Test"}]},
        "curves": [
            {"geometry": {"kind": "line", "start": [0.0, 1.0], "end": [2.0, 3.0]}, "visibility": "hidden", "source": "projected_model_edge"},
            {"geometry": {"kind": "circle", "center": [0.0, 0.0], "radius": 2.0}, "visibility": "visible", "source": "cosmetic"},
            {"geometry": {"kind": "arc", "center": [0.0, 0.0], "radius": 2.0, "start": [2.0, 0.0], "middle": [0.0, 2.0], "end": [-2.0, 0.0]}, "visibility": "visible", "source": "center_line"}
        ], "relationships": {}, "parameters": {}, "views": [], "assets": []});
        let drawing: Drawing = json::from_value(wire.clone()).unwrap();
        assert!(drawing.sheet.as_ref().unwrap().is_valid());
        assert!(drawing.curves.iter().all(super::DrawingCurve::is_valid));
        assert_eq!(json::to_value(drawing).unwrap(), wire);
        let mut rejected = wire.clone();
        rejected["sheet"]["width_mm"] = cadmpeg_core::json!(0);
        assert!(json::from_value::<Drawing>(rejected)
            .unwrap_err()
            .to_string()
            .contains("sheet"));
        let mut rejected = wire.clone();
        rejected["curves"][1]["geometry"]["radius"] = cadmpeg_core::json!(-1);
        assert!(json::from_value::<Drawing>(rejected).is_err());
        let mut rejected = wire.clone();
        rejected["curves"][0]["geometry"]["start"] = cadmpeg_core::json!([0, null]);
        assert!(json::from_value::<Drawing>(rejected).is_err());
        let mut rejected = wire;
        rejected["curves"][0]
            .as_object_mut()
            .unwrap()
            .remove("visibility");
        assert!(json::from_value::<Drawing>(rejected)
            .unwrap_err()
            .to_string()
            .contains("visibility"));
    }

    #[test]
    fn template_reference_preserves_string_wire_and_rejects_invalid_ids() {
        let value = cadmpeg_core::json!({"id": "synthetic:test:drawing#page", "object": "source", "kind": "page", "runtime_type": "Page", "order": 0, "template": "synthetic:test:drawing#template", "native_ref": "native", "relationships": {}, "curves": [], "parameters": {}, "views": [], "assets": []});
        let drawing: Drawing = json::from_value(value.clone()).expect("valid drawing");
        assert_eq!(
            drawing.template.as_ref().map(DrawingId::as_str),
            Some("synthetic:test:drawing#template")
        );
        assert_eq!(json::to_value(drawing).expect("serialize drawing"), value);
        for invalid in ["", "bad id"] {
            let mut invalid_value = value.clone();
            invalid_value["template"] = cadmpeg_core::json!(invalid);
            assert!(json::from_value::<Drawing>(invalid_value).is_err());
        }
    }
    #[test]
    fn views_are_required_and_a_base_view_is_written_only_where_stated() {
        let wire = cadmpeg_core::json!({"id": "synthetic:test:drawing#section", "object": "source", "kind": "section", "runtime_type": "Section", "order": 0,
            "relationships": {}, "views": ["synthetic:test:drawing#a", "synthetic:test:drawing#b"], "base_view": "synthetic:test:drawing#front",
            "curves": [], "parameters": {}, "assets": []});
        let drawing: Drawing = json::from_value(wire.clone()).expect("valid drawing");
        assert_eq!(drawing.views.len(), 2);
        assert_eq!(
            drawing.base_view.as_ref().map(DrawingId::as_str),
            Some("synthetic:test:drawing#front")
        );
        assert_eq!(json::to_value(drawing).expect("serialize"), wire);
        let mut missing = wire.clone();
        missing.as_object_mut().unwrap().remove("views");
        let error = json::from_value::<Drawing>(missing).expect_err("views are required");
        assert!(error.to_string().contains("views"), "{error}");
        for invalid in [serde_json::Value::Null, cadmpeg_core::json!("bad id")] {
            let mut rejected = wire.clone();
            rejected["base_view"] = invalid;
            let error = json::from_value::<Drawing>(rejected).expect_err("invalid base view");
            assert!(error.to_string().contains("base_view"), "{error}");
        }
        for invalid in [serde_json::Value::Null, cadmpeg_core::json!(["bad id"])] {
            let mut rejected = wire.clone();
            rejected["views"] = invalid;
            assert!(json::from_value::<Drawing>(rejected).is_err());
        }
    }

    #[test]
    fn numeric_admission_names_fields_and_preserves_valid_values() {
        let wire = cadmpeg_core::json!({"id": "synthetic:test:drawing#view", "object": "source", "kind": "view", "runtime_type": "View", "order": 0, "native_ref": "native", "position": [-2.0, 0.0], "scale": 0.5, "direction": [0.0, 0.0, 2.0], "rotation": -0.5, "relationships": {}, "curves": [], "parameters": {}, "views": [], "assets": []});
        let drawing: Drawing = json::from_value(wire.clone()).expect("valid drawing");
        assert_eq!(json::to_value(drawing).expect("serialize"), wire);
        for (field, invalid) in [
            ("scale", cadmpeg_core::json!(0)),
            ("direction", cadmpeg_core::json!([0, 0, 0])),
            ("position", cadmpeg_core::json!([0, null])),
        ] {
            let mut rejected = wire.clone();
            rejected[field] = invalid;
            let error = json::from_value::<Drawing>(rejected).expect_err("invalid numeric field");
            assert!(error.to_string().contains(field));
        }
    }

    #[test]
    fn view_projection_fields_round_trip_and_name_their_fields() {
        let wire = cadmpeg_core::json!({"id": "synthetic:test:drawing#view", "object": "source", "kind": "view", "runtime_type": "View", "order": 0,
            "scale_from": "fit", "projection": "third", "perspective": 250.0, "hidden_line": "polygonal",
            "relationships": {}, "curves": [], "parameters": {}, "views": [], "assets": []});
        let drawing: Drawing = json::from_value(wire.clone()).expect("valid drawing");
        assert!(drawing.native_ref.is_none());
        assert_eq!(drawing.scale_from, Some(super::ScaleFrom::Fit));
        assert_eq!(json::to_value(drawing).expect("serialize"), wire);
        for (field, invalid) in [
            ("scale_from", serde_json::Value::Null),
            ("projection", cadmpeg_core::json!("second")),
            ("perspective", cadmpeg_core::json!(0.0)),
            ("hidden_line", serde_json::Value::Null),
            ("native_ref", serde_json::Value::Null),
        ] {
            let mut rejected = wire.clone();
            rejected[field] = invalid;
            let error = json::from_value::<Drawing>(rejected).expect_err("invalid field");
            assert!(error.to_string().contains(field), "{field}: {error}");
        }
        let mut rejected = wire;
        rejected["rotation_degrees"] = cadmpeg_core::json!(90.0);
        assert!(json::from_value::<Drawing>(rejected).is_err());
    }

    /// §8: `edge_classes` is a set of classes drawn besides the visible hard
    /// edges, with the iso-line count; a class is stated once.
    #[test]
    fn edge_classes_round_trip_as_a_set() {
        let wire = cadmpeg_core::json!({"id": "synthetic:test:drawing#view", "object": "source", "kind": "view", "runtime_type": "View", "order": 0,
            "edge_classes": {"drawn": ["visible_smooth", "visible_iso", "hidden_hard"], "iso_count": 3},
            "relationships": {}, "curves": [], "parameters": {}, "views": [], "assets": []});
        let drawing: Drawing = json::from_value(wire.clone()).expect("valid drawing");
        let classes = drawing.edge_classes.as_ref().expect("edge classes");
        assert_eq!(
            classes.drawn.iter().copied().collect::<Vec<_>>(),
            [
                super::EdgeClass::VisibleSmooth,
                super::EdgeClass::VisibleIso,
                super::EdgeClass::HiddenHard
            ]
        );
        assert_eq!(json::to_value(drawing).expect("serialize"), wire);
        for (invalid, fragment) in [
            (serde_json::Value::Null, "edge_classes"),
            (
                cadmpeg_core::json!({"drawn": ["hidden_iso", "hidden_iso"]}),
                "",
            ),
            (cadmpeg_core::json!({"drawn": ["visible_hard"]}), ""),
            (
                cadmpeg_core::json!({"drawn": [], "iso_count": null}),
                "iso_count",
            ),
        ] {
            let mut rejected = wire.clone();
            rejected["edge_classes"] = invalid;
            let error = json::from_value::<Drawing>(rejected).expect_err("invalid field");
            assert!(error.to_string().contains(fragment), "{fragment}: {error}");
        }
    }

    #[test]
    fn a_free_form_map_refuses_a_blank_key() {
        let wire = cadmpeg_core::json!({
            "id": "synthetic:test:drawing#view",
            "object": "source",
            "kind": "view",
            "runtime_type": "View",
            "order": 0,
            "native_ref": "native",
            "parameters": {"   ": "value"}, "relationships": {}, "curves": [], "views": [], "assets": []
        });
        let error = json::from_value::<Drawing>(wire).expect_err("blank parameter role");
        assert!(error.to_string().contains("must not be blank"), "{error}");

        let relationships = cadmpeg_core::json!({
            "id": "synthetic:test:drawing#view",
            "object": "source",
            "kind": "view",
            "runtime_type": "View",
            "order": 0,
            "native_ref": "native",
            "relationships": {"": []}, "curves": [], "parameters": {}, "views": [], "assets": []
        });
        let error =
            json::from_value::<Drawing>(relationships).expect_err("blank relationship role");
        assert!(error.to_string().contains("must not be blank"), "{error}");

        let named = cadmpeg_core::json!({
            "id": "synthetic:test:drawing#view",
            "object": "source",
            "kind": "view",
            "runtime_type": "View",
            "order": 0,
            "native_ref": "native",
            "parameters": {" scale ": "value"}, "relationships": {}, "curves": [], "views": [], "assets": []
        });
        let drawing: Drawing = json::from_value(named).expect("a named role is kept verbatim");
        assert!(drawing.parameters.contains_key(" scale "));
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
cadmpeg_core::named_optional_field!(deserialize_template, DrawingId, "template");
cadmpeg_core::named_optional_field!(deserialize_base_view, DrawingId, "base_view");

mod identity_rewrite;

/// Sheet size and editable title-block content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DrawingSheet {
    /// Positive sheet width in millimetres.
    pub width_mm: crate::scalar::PositiveLength,
    /// Positive sheet height in millimetres.
    pub height_mm: crate::scalar::PositiveLength,
    /// Ordered name/value pairs; names are unique.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveVisibility {
    /// Visible geometry.
    Visible,
    /// Geometry behind a model face.
    Hidden,
}

/// Curve origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
        radius: crate::scalar::PositiveLength,
    },
    /// Circular arc from start through middle to end, with distinct points.
    Arc {
        /// Center point.
        center: crate::units::FiniteVector<2>,
        /// Positive radius in millimetres.
        radius: crate::scalar::PositiveLength,
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
            |radius: crate::scalar::PositiveLength| radius.get().is_finite() && radius.get() > 0.0;
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
                        ((x - cx).portable_hypot(y - cy) - radius.get()).abs()
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
    /// Font em size in millimetres, when specified.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_template_text_height"
    )]
    pub height: Option<crate::scalar::PositiveLength>,
}

cadmpeg_core::named_optional_field!(
    deserialize_template_text_height,
    crate::scalar::PositiveLength,
    "height"
);

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
