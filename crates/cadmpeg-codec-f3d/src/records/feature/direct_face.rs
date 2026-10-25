// SPDX-License-Identifier: Apache-2.0
//! Direct face operations: offset faces, shell, thicken, move and draft.

use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::scalar::{Angle, FiniteReal};
use serde::{Deserialize, Serialize};
/// Exact fixed-form construction data of a direct-face feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub(crate) enum DesignDirectFaceOperation {
    /// Signed normal offset applied to selected faces.
    OffsetFaces(DesignOffsetFacesOperation),
    /// Thin-wall shell applied after removing selected faces.
    Shell(DesignShellOperation),
    /// Signed normal thickness added from selected faces.
    Thicken(DesignThickenOperation),
}

/// Exact `OffsetFaces` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignOffsetFacesOperation {
    /// Signed distance in source centimetres.
    pub(crate) distance: FiniteReal,
    /// Referenced scalar record.
    pub(crate) distance_record_index: u32,
    /// Byte offset of the scalar of a located scope.
    #[serde(skip)]
    pub(crate) distance_offset: Option<u64>,
}

/// Exact `Shell` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellOperation {
    /// Wall thickness in source centimetres.
    pub(crate) thickness: cadmpeg_ir::scalar::PositiveReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar of a located scope.
    #[serde(skip)]
    pub(crate) thickness_offset: Option<u64>,
    /// Whether the wall grows outward from the original boundary.
    pub(crate) outward: bool,
    /// Byte offset of the outward Boolean of a located scope.
    #[serde(skip)]
    pub(crate) outward_offset: Option<u64>,
}

/// Exact `Thicken` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignThickenOperation {
    /// Signed thickness in source centimetres.
    pub(crate) signed_thickness: FiniteReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar of a located scope.
    #[serde(skip)]
    pub(crate) thickness_offset: Option<u64>,
}

/// Exact rigid transform carried by a Move feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignMoveOperation {
    /// Row-major model-space rigid transform in source centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar of a located scope.
    #[serde(skip)]
    pub(crate) transform_offset: Option<u64>,
    /// Indexed class-349 record carrying `transform`.
    pub(crate) transform_record_index: u32,
    /// Source transform-form discriminator.
    pub(crate) form: DesignMoveForm,
    /// Byte offset of `form` of a located scope.
    #[serde(skip)]
    pub(crate) form_offset: Option<u64>,
}

/// Source Move transform-form code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum DesignMoveForm {
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
pub(crate) struct DesignDraftOperation {
    /// Signed draft angle in radians.
    pub(crate) angle: Angle,
    /// Referenced draft-angle scalar record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the draft-angle scalar of a located scope.
    #[serde(skip)]
    pub(crate) angle_offset: Option<u64>,
    /// Zero-valued opposite-side angle scalar record.
    pub(crate) opposite_angle_record_index: u32,
    /// Byte offset of the opposite-side angle scalar of a located scope.
    #[serde(skip)]
    pub(crate) opposite_angle_offset: Option<u64>,
}

impl crate::records::source_offsets::SourceOffsets for DesignDirectFaceOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        match self {
            Self::OffsetFaces(operation) => operation.visit_source_offsets(offsets),
            Self::Shell(operation) => operation.visit_source_offsets(offsets),
            Self::Thicken(operation) => operation.visit_source_offsets(offsets),
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignOffsetFacesOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("distance_offset", &mut self.distance_offset);
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignShellOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("thickness_offset", &mut self.thickness_offset);
        offsets.offset("outward_offset", &mut self.outward_offset);
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignThickenOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("thickness_offset", &mut self.thickness_offset);
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignMoveOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("transform_offset", &mut self.transform_offset);
        offsets.offset("form_offset", &mut self.form_offset);
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignDraftOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("angle_offset", &mut self.angle_offset);
        offsets.offset("opposite_angle_offset", &mut self.opposite_angle_offset);
    }
}

mod identity_rewrite;
