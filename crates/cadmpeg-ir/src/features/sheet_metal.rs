use super::{FaceSelection, PathRef};
use crate::scalar::{Angle, PositiveLength};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Independently decoded height target and offset inputs."]
pub struct SheetMetalFlangeTargetConstruction {
    #[doc = "Height reference target."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_target_inputs_target"
    )]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "super::SheetMetalFlangeHeightTarget")
    )]
    pub target: Option<super::SheetMetalFlangeHeightTarget>,
    #[doc = "Signed target offset in millimetres."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_target_inputs_offset"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "crate::scalar::Length"))]
    pub offset: Option<crate::scalar::Length>,
}
cadmpeg_core::named_optional_field!(
    deserialize_edge_target_inputs_target,
    super::SheetMetalFlangeHeightTarget,
    "target"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_target_inputs_offset,
    crate::scalar::Length,
    "offset"
);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Independently decoded symmetric flange width inputs."]
pub struct SheetMetalFlangeSymmetricWidth {
    #[doc = "Total symmetric width in millimetres."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_symmetric_width_width"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "PositiveLength"))]
    pub width: Option<PositiveLength>,
}
cadmpeg_core::named_optional_field!(
    deserialize_edge_symmetric_width_width,
    PositiveLength,
    "width"
);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Independently decoded inputs of an edge flange."]
pub struct SheetMetalEdgeFlangeConstruction {
    #[doc = "Selected source edges."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_edges"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::EdgeSelection"))]
    pub edges: Option<super::EdgeSelection>,
    #[doc = "Height from the selected datum."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_height"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::SheetMetalFlangeHeight"))]
    pub height: Option<super::SheetMetalFlangeHeight>,
    #[doc = "Wall angle in radians."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_angle"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Angle"))]
    pub angle: Option<Angle>,
    #[doc = "Faces from which the height is measured."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_height_datum"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::SheetMetalHeightDatum"))]
    pub height_datum: Option<super::SheetMetalHeightDatum>,
    #[doc = "Bend placement against the source edges."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_bend_position"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::SheetMetalBendPosition"))]
    pub bend_position: Option<super::SheetMetalBendPosition>,
    #[doc = "Extent along each selected edge."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_width"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::SheetMetalFlangeWidth"))]
    pub width: Option<super::SheetMetalFlangeWidth>,
    #[doc = "Inside bend radius in millimetres."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_bend_radius"
    )]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "crate::scalar::NonNegativeLength")
    )]
    pub bend_radius: Option<crate::scalar::NonNegativeLength>,
}
cadmpeg_core::named_optional_field!(deserialize_edge_edges, super::EdgeSelection, "edges");
cadmpeg_core::named_optional_field!(
    deserialize_edge_height,
    super::SheetMetalFlangeHeight,
    "height"
);
cadmpeg_core::named_optional_field!(deserialize_edge_angle, Angle, "angle");
cadmpeg_core::named_optional_field!(
    deserialize_edge_height_datum,
    super::SheetMetalHeightDatum,
    "height_datum"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_bend_position,
    super::SheetMetalBendPosition,
    "bend_position"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_width,
    super::SheetMetalFlangeWidth,
    "width"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_bend_radius,
    crate::scalar::NonNegativeLength,
    "bend_radius"
);

impl SheetMetalEdgeFlangeConstruction {
    #[doc = "Whether all required construction inputs are present."]
    pub fn has_all_inputs(&self) -> bool {
        self.edges.is_some()
            && self.height.as_ref().is_some_and(|height| match height {
                super::SheetMetalFlangeHeight::ToObject(construction) => {
                    construction.target.is_some() && construction.offset.is_some()
                }
                super::SheetMetalFlangeHeight::Distance(_) => true,
            })
            && self.angle.is_some()
            && self.height_datum.is_some()
            && self.bend_position.is_some()
            && self.bend_radius.is_some()
            && self.width.as_ref().is_some_and(|width| match width {
                super::SheetMetalFlangeWidth::SymmetricPerEdge { widths } => {
                    widths.iter().all(|row| row.width.is_some())
                }
                super::SheetMetalFlangeWidth::TwoSidesPerEdge { widths } => widths
                    .as_slice()
                    .iter()
                    .all(|row| row.first.is_some() && row.second.is_some()),
                super::SheetMetalFlangeWidth::Symmetric(width) => width.width.is_some(),
                super::SheetMetalFlangeWidth::TwoSides(width) => {
                    width.first.is_some() && width.second.is_some()
                }
                super::SheetMetalFlangeWidth::TwoOffsetsPerEdge { offsets } => {
                    offsets.iter().all(|row| {
                        row.first.is_some()
                            && row.second.is_some()
                            && row.first_reference.is_some()
                            && row.second_reference.is_some()
                    })
                }
                super::SheetMetalFlangeWidth::FullEdge => true,
            })
    }
    #[doc = "Construct an edge flange with all seven construction inputs."]
    pub fn complete(
        edges: super::EdgeSelection,
        height: super::SheetMetalFlangeHeight,
        angle: Angle,
        height_datum: super::SheetMetalHeightDatum,
        bend_position: super::SheetMetalBendPosition,
        width: super::SheetMetalFlangeWidth,
        bend_radius: crate::scalar::NonNegativeLength,
    ) -> Self {
        Self {
            edges: Some(edges),
            height: Some(height),
            angle: Some(angle),
            height_datum: Some(height_datum),
            bend_position: Some(bend_position),
            width: Some(width),
            bend_radius: Some(bend_radius),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Signed offsets and boundary reference programs for one selected edge."]
pub struct SheetMetalFlangeOffset {
    #[doc = "First signed offset in millimetres; positive extends and negative narrows the flange."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_offset_first"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "crate::scalar::Length"))]
    pub first: Option<crate::scalar::Length>,
    #[doc = "Second signed offset in millimetres; positive extends and negative narrows the flange."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_offset_second"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "crate::scalar::Length"))]
    pub second: Option<crate::scalar::Length>,
    #[doc = "First boundary reference program, when decoded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_offset_first_reference"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::SelectionReference"))]
    pub first_reference: Option<super::SelectionReference>,
    #[doc = "Second boundary reference program, when decoded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_edge_offset_second_reference"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::SelectionReference"))]
    pub second_reference: Option<super::SelectionReference>,
}
cadmpeg_core::named_optional_field!(
    deserialize_edge_offset_first,
    crate::scalar::Length,
    "first"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_offset_second,
    crate::scalar::Length,
    "second"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_offset_first_reference,
    super::SelectionReference,
    "first_reference"
);
cadmpeg_core::named_optional_field!(
    deserialize_edge_offset_second_reference,
    super::SelectionReference,
    "second_reference"
);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Inputs of a sheet-metal bend along sketch lines."]
pub struct SheetMetalBendLinesConstruction {
    #[doc = "Line bends in source selection order."]
    pub bends: Vec<SheetMetalLineBend>,
    #[doc = "Face kept stationary during bending, when decoded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stationary"
    )]
    pub stationary: Option<FaceSelection>,
    #[doc = "Inside bend radius in millimetres, when decoded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inside_radius"
    )]
    pub inside_radius: Option<PositiveLength>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "One sketch-line selection and its signed bend angle."]
pub struct SheetMetalLineBend {
    #[doc = "Sketch line or retained source selection."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_line"
    )]
    pub line: Option<PathRef>,
    #[doc = "Signed angle in radians, when decoded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Angle>,
}

cadmpeg_core::named_optional_field!(deserialize_stationary, FaceSelection, "stationary");
cadmpeg_core::named_optional_field!(deserialize_inside_radius, PositiveLength, "inside_radius");
cadmpeg_core::named_optional_field!(deserialize_line, PathRef, "line");
cadmpeg_core::named_optional_field!(deserialize_angle, Angle, "angle");

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Inputs for flattening selected bends of an existing sheet."]
pub struct SheetMetalUnfoldConstruction {
    #[doc = "Face kept stationary while unfolding."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stationary"
    )]
    pub stationary: Option<FaceSelection>,
    #[doc = "Selected bend faces."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bends"
    )]
    pub bends: Option<FaceSelection>,
    #[doc = "Whether all bends are unfolded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_all_bends"
    )]
    pub all_bends: Option<bool>,
    #[doc = "Neutral-axis offset divided by sheet thickness."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_neutral_factor"
    )]
    pub neutral_factor: Option<crate::scalar::FiniteReal>,
}

cadmpeg_core::named_optional_field!(deserialize_bends, FaceSelection, "bends");
cadmpeg_core::named_optional_field!(deserialize_all_bends, bool, "all_bends");
cadmpeg_core::named_optional_field!(
    deserialize_neutral_factor,
    crate::scalar::FiniteReal,
    "neutral_factor"
);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Inputs for restoring the bends flattened by an Unfold."]
pub struct SheetMetalRefoldConstruction {
    #[doc = "Preceding Unfold whose bends are restored."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unfold"
    )]
    pub unfold: Option<super::FeatureId>,
}

cadmpeg_core::named_optional_field!(deserialize_unfold, super::FeatureId, "unfold");

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Construction of a sheet from open profiles with rounded bends and linear travel."]
pub struct SheetMetalContourFlangeConstruction {
    #[doc = "Selected open profile program."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_profile"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::PlanarProfileRef"))]
    pub profile: Option<super::PlanarProfileRef>,
    #[doc = "Travel direction in model coordinates."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_direction"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::FeatureDirection3"))]
    pub direction: Option<super::FeatureDirection3>,
    #[doc = "Signed travel from the profile plane."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_extent"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "super::ExtrudeExtent"))]
    pub extent: Option<super::ExtrudeExtent>,
    #[doc = "Finished sheet thickness in millimetres."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_thickness"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "PositiveLength"))]
    pub thickness: Option<PositiveLength>,
    #[doc = "Inside bend radius in millimetres."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_inside_radius"
    )]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "crate::scalar::NonNegativeLength")
    )]
    pub inside_radius: Option<crate::scalar::NonNegativeLength>,
    #[doc = "Thickness distribution across the oriented profile."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_side"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "SheetMetalContourThicknessSide"))]
    pub side: Option<SheetMetalContourThicknessSide>,
    #[doc = "Neutral-axis offset divided by sheet thickness."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_contour_neutral_factor"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "crate::scalar::FiniteReal"))]
    pub neutral_factor: Option<crate::scalar::FiniteReal>,
}

cadmpeg_core::named_optional_field!(
    deserialize_contour_profile,
    super::PlanarProfileRef,
    "profile"
);
cadmpeg_core::named_optional_field!(
    deserialize_contour_direction,
    super::FeatureDirection3,
    "direction"
);
cadmpeg_core::named_optional_field!(deserialize_contour_extent, super::ExtrudeExtent, "extent");
cadmpeg_core::named_optional_field!(deserialize_contour_thickness, PositiveLength, "thickness");
cadmpeg_core::named_optional_field!(
    deserialize_contour_inside_radius,
    crate::scalar::NonNegativeLength,
    "inside_radius"
);
cadmpeg_core::named_optional_field!(
    deserialize_contour_side,
    SheetMetalContourThicknessSide,
    "side"
);
cadmpeg_core::named_optional_field!(
    deserialize_contour_neutral_factor,
    crate::scalar::FiniteReal,
    "neutral_factor"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[doc = "Sheet thickness relative to the profile's plane normal and ordered tangent."]
pub enum SheetMetalContourThicknessSide {
    #[doc = "Thickness extends along the profile-plane normal crossed with its tangent."]
    Left,
    #[doc = "Thickness extends opposite the profile-plane normal crossed with its tangent."]
    Right,
    #[doc = "Half the thickness extends to each side of the profile."]
    Centered,
}

#[cfg(test)]
mod tests;
