use super::{FaceSelection, PathRef};
use crate::scalar::{Angle, PositiveLength};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
