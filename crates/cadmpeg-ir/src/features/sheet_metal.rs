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

#[cfg(test)]
mod tests;
