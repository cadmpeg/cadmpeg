// SPDX-License-Identifier: Apache-2.0
//! Solid primitives: box, cylinder, sphere and torus.

use super::extrude::DesignExtrudeOperation;
use super::scope::DesignScopePayload;
use crate::records::identity::Located;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");
cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");
/// Exact construction data of a solid primitive scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "primitive")]
pub enum DesignSolidPrimitive {
    /// Axis-aligned box defined by five owned dimensions and offsets.
    Box(DesignBoxPrimitive),
    /// Circular cylinder defined by height and diameter owners.
    Cylinder(DesignCylinderPrimitive),
    /// Sphere defined by a placement frame and diameter.
    Sphere(DesignSpherePrimitive),
    /// Torus defined by a placement frame and two diameters.
    Torus(DesignTorusPrimitive),
}

/// Exact `Box` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignBoxPrimitive {
    /// Length along the source x-axis in source centimetres.
    pub length: f64,
    /// Referenced length owner.
    pub length_record_index: u32,
    /// Byte offset of the evaluated length.
    pub length_offset: u64,
    /// Width along the source y-axis in source centimetres.
    pub width: f64,
    /// Referenced width owner.
    pub width_record_index: u32,
    /// Byte offset of the evaluated width.
    pub width_offset: u64,
    /// Height along the source z-axis in source centimetres.
    pub height: f64,
    /// Referenced height owner.
    pub height_record_index: u32,
    /// Byte offset of the evaluated height.
    pub height_offset: u64,
    /// Translation along the source x-axis in source centimetres.
    pub offset_x: f64,
    /// Referenced x-offset owner.
    pub offset_x_record_index: u32,
    /// Byte offset of the evaluated x offset.
    pub offset_x_offset: u64,
    /// Translation along the source y-axis in source centimetres.
    pub offset_y: f64,
    /// Referenced y-offset owner.
    pub offset_y_record_index: u32,
    /// Byte offset of the evaluated y offset.
    pub offset_y_offset: u64,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

/// Exact `Cylinder` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCylinderPrimitiveWire",
    into = "DesignCylinderPrimitiveWire"
)]
pub struct DesignCylinderPrimitive {
    /// Axial height in source centimetres.
    pub height: f64,
    /// Referenced height owner.
    pub height_record_index: u32,
    /// Byte offset of the evaluated height.
    pub height_offset: u64,
    /// Circular diameter in source centimetres.
    pub diameter: f64,
    /// Referenced diameter owner.
    pub diameter_record_index: u32,
    /// Byte offset of the evaluated diameter.
    pub diameter_offset: u64,
    /// Source frame carried by the shifted cylinder form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Located<SketchPlacementMatrix>>,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignCylinderPrimitiveWire {
    /// Axial height in source centimetres.
    height: f64,
    /// Referenced height owner.
    height_record_index: u32,
    /// Byte offset of the evaluated height.
    height_offset: u64,
    /// Circular diameter in source centimetres.
    diameter: f64,
    /// Referenced diameter owner.
    diameter_record_index: u32,
    /// Byte offset of the evaluated diameter.
    diameter_offset: u64,
    /// Source frame carried by the shifted cylinder form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the shifted-form source frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Result Boolean operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    operation_offset: u64,
}

impl From<DesignCylinderPrimitive> for DesignCylinderPrimitiveWire {
    fn from(value: DesignCylinderPrimitive) -> Self {
        Self {
            height: value.height,
            height_record_index: value.height_record_index,
            height_offset: value.height_offset,
            diameter: value.diameter,
            diameter_record_index: value.diameter_record_index,
            diameter_offset: value.diameter_offset,
            transform: value.transform.map(|located| located.value),
            transform_offset: value.transform.map(|located| located.offset),
            operation: value.operation,
            operation_offset: value.operation_offset,
        }
    }
}

impl TryFrom<DesignCylinderPrimitiveWire> for DesignCylinderPrimitive {
    type Error = String;
    fn try_from(value: DesignCylinderPrimitiveWire) -> Result<Self, Self::Error> {
        Ok(Self {
            height: value.height,
            height_record_index: value.height_record_index,
            height_offset: value.height_offset,
            diameter: value.diameter,
            diameter_record_index: value.diameter_record_index,
            diameter_offset: value.diameter_offset,
            transform: Located::from_wire(value.transform, value.transform_offset, "transform")?,
            operation: value.operation,
            operation_offset: value.operation_offset,
        })
    }
}

/// Exact `Sphere` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSpherePrimitive {
    /// Row-major local-to-model placement frame.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the placement matrix.
    pub transform_offset: u64,
    /// Sphere diameter in source centimetres.
    pub diameter: f64,
    /// Referenced diameter record.
    pub diameter_record_index: u32,
    /// Byte offset of the diameter scalar.
    pub diameter_offset: u64,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

/// Exact `Torus` primitive construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignTorusPrimitive {
    /// Row-major local-to-model placement frame.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the placement matrix.
    pub transform_offset: u64,
    /// Major diameter in source centimetres.
    pub major_diameter: f64,
    /// Referenced major-diameter record.
    pub major_diameter_record_index: u32,
    /// Byte offset of the major-diameter scalar.
    pub major_diameter_offset: u64,
    /// Tube diameter in source centimetres.
    pub minor_diameter: f64,
    /// Referenced minor-diameter record.
    pub minor_diameter_record_index: u32,
    /// Byte offset of the minor-diameter scalar.
    pub minor_diameter_offset: u64,
    /// Result Boolean operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation enum.
    pub operation_offset: u64,
}

impl From<DesignSolidPrimitive> for DesignScopePayload {
    fn from(value: DesignSolidPrimitive) -> Self {
        match value {
            DesignSolidPrimitive::Box(value) => Self::BoxPrimitive(Some(value)),
            DesignSolidPrimitive::Cylinder(value) => Self::CylinderPrimitive(Some(value)),
            DesignSolidPrimitive::Sphere(value) => Self::SpherePrimitive(Some(value)),
            DesignSolidPrimitive::Torus(value) => Self::TorusPrimitive(Some(value)),
        }
    }
}
