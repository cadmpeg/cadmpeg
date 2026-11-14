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
