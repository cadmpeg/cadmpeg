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
    pub stationary: Option<FaceSelection>,
    #[doc = "Inside bend radius in millimetres, when decoded."]
    pub inside_radius: Option<PositiveLength>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "One sketch-line selection and its signed bend angle."]
pub struct SheetMetalLineBend {
    #[doc = "Sketch line or retained source selection."]
    pub line: Option<PathRef>,
    #[doc = "Signed angle in radians, when decoded."]
    pub angle: Option<Angle>,
}

#[doc = "Distribution of sheet thickness relative to its construction plane."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalThicknessSide {
    #[doc = "Thickness lies along the profile plane's positive normal."]
    Forward,
    #[doc = "Thickness lies along the profile plane's negative normal."]
    Reverse,
    #[doc = "Thickness is split equally across both sides of the profile plane."]
    Symmetric,
}
