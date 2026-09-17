// SPDX-License-Identifier: Apache-2.0
//! Direct face operations: offset faces, shell, thicken, move and draft.

use super::sheet_metal::DesignFiniteScalar;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::{Deserialize, Serialize};
/// Exact fixed-form construction data of a direct-face feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum DesignDirectFaceOperation {
    /// Signed normal offset applied to selected faces.
    OffsetFaces(DesignOffsetFacesOperation),
    /// Thin-wall shell applied after removing selected faces.
    Shell(DesignShellOperation),
    /// Signed normal thickness added from selected faces.
    Thicken(DesignThickenOperation),
}

/// Exact `OffsetFaces` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignOffsetFacesOperation {
    /// Signed distance in source centimetres.
    pub distance: f64,
    /// Referenced scalar record.
    pub distance_record_index: u32,
    /// Byte offset of the scalar.
    pub distance_offset: u64,
}

/// Exact `Shell` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignShellOperation {
    /// Positive wall thickness in source centimetres.
    pub thickness: f64,
    /// Referenced scalar record.
    pub thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub thickness_offset: u64,
    /// Whether the wall grows outward from the original boundary.
    pub outward: bool,
    /// Byte offset of the outward Boolean.
    pub outward_offset: u64,
}

/// Exact `Thicken` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignThickenOperation {
    /// Signed thickness in source centimetres.
    pub signed_thickness: f64,
    /// Referenced scalar record.
    pub thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub thickness_offset: u64,
}

/// Exact rigid transform carried by a Move feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignMoveOperation {
    /// Row-major model-space rigid transform in source centimetres.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
    /// Indexed class-349 record carrying `transform`.
    pub transform_record_index: u32,
    /// Source transform-form discriminator.
    pub form: DesignMoveForm,
    /// Byte offset of `form`.
    pub form_offset: u64,
}

/// Source Move transform-form code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub enum DesignMoveForm {
    /// Source form 1.
    Form1,
    /// Source form 5.
    Form5,
}

impl TryFrom<u32> for DesignMoveForm {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Form1),
            5 => Ok(Self::Form5),
            _ => Err(format!("form must be 1 or 5, not {value}")),
        }
    }
}

impl From<DesignMoveForm> for u32 {
    fn from(form: DesignMoveForm) -> Self {
        match form {
            DesignMoveForm::Form1 => 1,
            DesignMoveForm::Form5 => 5,
        }
    }
}

/// Exact signed-angle lanes carried by a `Draft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignDraftOperation {
    /// Signed draft angle in radians.
    pub angle: DesignFiniteScalar,
    /// Referenced draft-angle scalar record.
    pub angle_record_index: u32,
    /// Byte offset of the draft-angle scalar.
    pub angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record.
    pub opposite_angle_record_index: u32,
    /// Byte offset of the opposite-side angle scalar.
    pub opposite_angle_offset: u64,
}
