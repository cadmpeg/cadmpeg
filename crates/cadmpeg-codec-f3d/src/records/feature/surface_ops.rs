// SPDX-License-Identifier: Apache-2.0
//! Surface operations: stitch, extend, offset, trim, rule and patch.

use super::sheet_metal::DesignPositiveScalar;
use crate::records::{
    identity::NonEmptyVec, mesh::DesignRelaxedGuidText, references::DesignClassTag,
};
use serde::{Deserialize, Serialize};
/// Fixed operation records named by a `SurfaceStitch` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSurfaceStitchOperation {
    /// Positive maximum stitched-boundary gap in centimetres.
    pub gap_tolerance: DesignPositiveScalar,
    /// Byte offset of `gap_tolerance`.
    pub gap_tolerance_offset: u64,
    /// Indexed tolerance-record identity.
    pub tolerance_record_index: u32,
    /// Indexed operation-settings record identity.
    pub settings_record_index: u32,
}

/// Geometric continuation law encoded by a `SurfaceExtend` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignSurfaceExtendMethod {
    /// Continue the source surface parameterization.
    Natural,
    /// Create faces tangent to the source faces.
    Tangent,
    /// Create faces perpendicular to the source faces.
    Perpendicular,
}

/// Fixed construction records named by a `SurfaceExtend` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSurfaceExtendOperation {
    /// Extension distance in source centimetres.
    pub distance: f64,
    /// Byte offset of `distance`.
    pub distance_offset: u64,
    /// Indexed scalar record carrying `distance`.
    pub distance_record_index: u32,
    /// Geometric continuation law.
    pub method: DesignSurfaceExtendMethod,
    /// Byte offset of the method enum.
    pub method_offset: u64,
    /// Indexed boundary-carrier record.
    pub boundary_record_index: u32,
    /// Additional indexed reference carried by the boundary tail.
    pub boundary_reference_record_index: u32,
    /// Byte offset of `boundary_reference_record_index`'s marked reference.
    pub boundary_reference_offset: u64,
    /// Ordered edge-recipe records contained by the boundary carrier.
    pub edge_record_indices: Vec<u32>,
    /// Positive modelling tolerance in source centimetres.
    pub tolerance: f64,
    /// Byte offset of `tolerance`.
    pub tolerance_offset: u64,
}

/// Source selection form named by a `SurfaceOffset` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DesignSurfaceOffsetSupport {
    /// A boundary carrier followed by edge recipes.
    BoundaryCarrier {
        /// Indexed boundary-carrier record.
        boundary_record_index: u32,
        /// Additional indexed reference carried by the boundary tail.
        boundary_reference_record_index: u32,
        /// Byte offset of `boundary_reference_record_index`'s marked reference.
        boundary_reference_offset: u64,
        /// Ordered edge-recipe records contained by the boundary carrier.
        edge_record_indices: Vec<u32>,
        /// Positive modelling tolerance in source centimetres.
        tolerance: f64,
        /// Byte offset of `tolerance`.
        tolerance_offset: u64,
    },
    /// Counted role-0x41 groups containing bounded-face recipes.
    FaceGroups {
        /// Ordered construction-group records named by the scope.
        group_record_indices: Vec<u32>,
    },
}

/// Fixed construction records named by a `SurfaceOffset` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSurfaceOffsetOperation {
    /// Signed offset distance in source centimetres.
    pub distance: f64,
    /// Byte offset of `distance`.
    pub distance_offset: u64,
    /// Indexed scalar record carrying `distance`.
    pub distance_record_index: u32,
    /// Exact source selection form.
    pub support: DesignSurfaceOffsetSupport,
}

/// One indexed record in the auxiliary chain preceding a `SurfaceTrim` cell table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSurfaceTrimChainRecord {
    /// Indexed record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Source per-file dynamic class tag.
    pub class_tag: DesignClassTag,
    /// Bytes from the primary header to the following indexed header.
    pub frame_length: u64,
}

/// One source `BRep` cell entry in a `SurfaceTrim` cell table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSurfaceTrimCellEntry {
    /// Indexed cell-record identity.
    pub record_index: u32,
    /// Byte offset of the marked cell-record reference.
    pub record_reference_offset: u64,
    /// One-based partition ordinal of a cell selected for removal.
    pub ordinal: u64,
    /// Byte offset of the serialized entry ordinal.
    pub ordinal_offset: u64,
}

/// Exact auxiliary carrier of a `SurfaceTrim` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSurfaceTrimOperationWire",
    into = "DesignSurfaceTrimOperationWire"
)]
pub struct DesignSurfaceTrimOperation {
    /// Globally unique deterministic identifier for this native carrier.
    pub id: String,
    /// Owning `SurfaceTrim` parameter-scope record index.
    pub scope_record_index: u32,
    /// Indexed entity-selection record that starts the trimming tool chain.
    pub selection_record_index: u32,
    /// Byte offset of the entity-selection record.
    pub selection_byte_offset: u64,
    /// Indexed record immediately following the entity-selection frame.
    pub selection_next_record_index: u32,
    /// Byte offset of the record immediately following the entity-selection frame.
    pub selection_next_byte_offset: u64,
    /// Two indexed records between the entity selection and the cell table.
    pub chain_records: [DesignSurfaceTrimChainRecord; 2],
    /// Indexed record carrying the counted BRep-cell table.
    pub cell_table_record_index: u32,
    /// Byte offset of the cell-table primary header.
    pub cell_table_byte_offset: u64,
    /// Dynamic class tag of the cell-table primary frame.
    pub cell_table_class_tag: DesignClassTag,
    /// Bytes from the cell-table primary header to its paired header.
    pub cell_table_frame_length: u64,
    /// Dynamic class tag of the cell-table paired frame.
    pub cell_table_paired_class_tag: DesignClassTag,
    /// Byte offset of the cell-table paired header.
    pub cell_table_paired_byte_offset: u64,
    /// Byte offset of the cell-table count.
    pub cell_count_offset: u64,
    /// Ordered cell-table entries.
    cell_entries: NonEmptyVec<DesignSurfaceTrimCellEntry>,
    /// Total number of cells in the operation's partition.
    pub trailing_value: u32,
    /// Byte offset of `trailing_value`.
    pub trailing_value_offset: u64,
    /// Byte offset of the zero value after `trailing_value`.
    pub trailing_zero_offset: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DesignSurfaceTrimOperationWire {
    /// Globally unique deterministic identifier for this native carrier.
    pub id: String,
    /// Owning `SurfaceTrim` parameter-scope record index.
    pub scope_record_index: u32,
    /// Indexed entity-selection record that starts the trimming tool chain.
    pub selection_record_index: u32,
    /// Byte offset of the entity-selection record.
    pub selection_byte_offset: u64,
    /// Indexed record immediately following the entity-selection frame.
    pub selection_next_record_index: u32,
    /// Byte offset of the record immediately following the entity-selection frame.
    pub selection_next_byte_offset: u64,
    /// Two indexed records between the entity selection and the cell table.
    pub chain_records: [DesignSurfaceTrimChainRecord; 2],
    /// Indexed record carrying the counted BRep-cell table.
    pub cell_table_record_index: u32,
    /// Byte offset of the cell-table primary header.
    pub cell_table_byte_offset: u64,
    /// Dynamic class tag of the cell-table primary frame.
    pub cell_table_class_tag: DesignClassTag,
    /// Bytes from the cell-table primary header to its paired header.
    pub cell_table_frame_length: u64,
    /// Dynamic class tag of the cell-table paired frame.
    pub cell_table_paired_class_tag: DesignClassTag,
    /// Byte offset of the cell-table paired header.
    pub cell_table_paired_byte_offset: u64,
    /// Byte offset of the cell-table count.
    pub cell_count_offset: u64,
    /// Ordered cell-table entries.
    pub cell_entries: Vec<DesignSurfaceTrimCellEntry>,
    /// Total number of cells in the operation's partition.
    pub trailing_value: u32,
    /// Byte offset of `trailing_value`.
    pub trailing_value_offset: u64,
    /// Byte offset of the zero value after `trailing_value`.
    pub trailing_zero_offset: u64,
}

impl TryFrom<DesignSurfaceTrimOperationWire> for DesignSurfaceTrimOperation {
    type Error = &'static str;
    fn try_from(wire: DesignSurfaceTrimOperationWire) -> Result<Self, Self::Error> {
        let cell_entries =
            NonEmptyVec::new(wire.cell_entries).ok_or("cell_entries must not be empty")?;
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            selection_record_index: wire.selection_record_index,
            selection_byte_offset: wire.selection_byte_offset,
            selection_next_record_index: wire.selection_next_record_index,
            selection_next_byte_offset: wire.selection_next_byte_offset,
            chain_records: wire.chain_records,
            cell_table_record_index: wire.cell_table_record_index,
            cell_table_byte_offset: wire.cell_table_byte_offset,
            cell_table_class_tag: wire.cell_table_class_tag,
            cell_table_frame_length: wire.cell_table_frame_length,
            cell_table_paired_class_tag: wire.cell_table_paired_class_tag,
            cell_table_paired_byte_offset: wire.cell_table_paired_byte_offset,
            cell_count_offset: wire.cell_count_offset,
            cell_entries,
            trailing_value: wire.trailing_value,
            trailing_value_offset: wire.trailing_value_offset,
            trailing_zero_offset: wire.trailing_zero_offset,
        })
    }
}

impl From<DesignSurfaceTrimOperation> for DesignSurfaceTrimOperationWire {
    fn from(value: DesignSurfaceTrimOperation) -> Self {
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            selection_record_index: value.selection_record_index,
            selection_byte_offset: value.selection_byte_offset,
            selection_next_record_index: value.selection_next_record_index,
            selection_next_byte_offset: value.selection_next_byte_offset,
            chain_records: value.chain_records,
            cell_table_record_index: value.cell_table_record_index,
            cell_table_byte_offset: value.cell_table_byte_offset,
            cell_table_class_tag: value.cell_table_class_tag,
            cell_table_frame_length: value.cell_table_frame_length,
            cell_table_paired_class_tag: value.cell_table_paired_class_tag,
            cell_table_paired_byte_offset: value.cell_table_paired_byte_offset,
            cell_count_offset: value.cell_count_offset,
            cell_entries: value.cell_entries.into_vec(),
            trailing_value: value.trailing_value,
            trailing_value_offset: value.trailing_value_offset,
            trailing_zero_offset: value.trailing_zero_offset,
        }
    }
}

impl DesignSurfaceTrimOperation {
    pub(crate) fn cell_entries(&self) -> &[DesignSurfaceTrimCellEntry] {
        self.cell_entries.as_slice()
    }
}

/// Direction law encoded by a `SurfaceRuled` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignRuledSurfaceMethod {
    /// Generate ruled strips tangent to the support faces.
    Tangent,
    /// Generate ruled strips normal to the support faces.
    Normal,
    /// Generate ruled strips along an explicitly selected direction.
    Direction,
}

/// Corner law encoded by a `SurfaceRuled` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignRuledSurfaceCorner {
    /// Round adjacent ruled strips through a common corner.
    Rounded,
    /// Intersect adjacent ruled strips at a miter.
    Mitered,
}

/// Fixed construction carried by a `SurfaceRuled` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignRuledSurfaceOperation {
    /// Direction law.
    pub method: DesignRuledSurfaceMethod,
    /// Byte offset of the direction-law enum.
    pub method_offset: u64,
    /// Corner construction law.
    pub corner: DesignRuledSurfaceCorner,
    /// Byte offset of the corner-law enum.
    pub corner_offset: u64,
    /// Whether the opposite incident face supplies the angle reference.
    pub alternate_face: bool,
    /// Byte offset of the alternate-face Boolean.
    pub alternate_face_offset: u64,
    /// Referenced ruled-angle parameter owner.
    pub angle_owner_record_index: u32,
    /// Referenced ruled-distance parameter owner.
    pub distance_owner_record_index: u32,
    /// Ordered role-`0x08` edge-group records.
    pub edge_group_record_indices: Vec<u32>,
    /// Ordered auxiliary selection records between the edge-group runs.
    pub auxiliary_record_indices: Vec<u32>,
    /// Serialized direction entity identity; the all-zero UUID means absent.
    pub direction_entity_id: Option<DesignRelaxedGuidText>,
}

/// Boundary condition a `SurfacePatch` component imposes against its adjacent
/// face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignPatchContinuity {
    /// Positional continuity only.
    Connected,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
    /// A serialized value whose continuity meaning is not settled.
    Unknown(u32),
}

impl DesignPatchContinuity {
    /// Decode the serialized continuity ordinal without discarding unknown values.
    #[must_use]
    pub fn from_code(code: u32) -> Self {
        match code {
            0 => Self::Connected,
            1 => Self::Tangent,
            2 => Self::Curvature,
            code => Self::Unknown(code),
        }
    }
}

/// Pipe generated-section shape selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum DesignPipeSectionShape {
    Circular,
    Unknown(u8),
}

impl DesignPipeSectionShape {
    #[must_use]
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Circular,
            code => Self::Unknown(code),
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Circular => 1,
            Self::Unknown(code) => code,
        }
    }
}

impl From<u8> for DesignPipeSectionShape {
    fn from(code: u8) -> Self {
        Self::from_code(code)
    }
}

impl From<DesignPipeSectionShape> for u8 {
    fn from(shape: DesignPipeSectionShape) -> Self {
        shape.code()
    }
}

/// Settings a `SurfacePatch` scope carries for one boundary component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSurfacePatchBoundary {
    /// Position of the settings record in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Indexed settings-record identity.
    pub record_index: u32,
    /// Source `IsSeedSel` flag.
    pub is_seed_selection: bool,
    /// Boundary condition this component imposes against its adjacent face.
    pub continuity: DesignPatchContinuity,
    /// Source `PatchFlip` ordinal. Retained without a neutral meaning.
    pub flip: u32,
    /// Source `PatchScale` value.
    pub scale: f64,
    /// Indexed record the `rPatchModelRef` reference names: this boundary
    /// component's model reference.
    pub model_reference: u32,
}
