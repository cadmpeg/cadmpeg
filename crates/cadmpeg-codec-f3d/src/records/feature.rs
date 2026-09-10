// SPDX-License-Identifier: Apache-2.0
//! Typed modeling-feature scopes, operations, and source operands.

use super::topology::{DesignEntitySelectionFaceCandidate, DesignSketchProfileOperand};
use super::SketchPlacementMatrix;
use super::{deserialize_absent_u64_offset, serialize_absent_u64_offset};
use super::{
    ConstructionRecipeDesign, ConstructionRecipeKind, ConstructionRecipeSelector, DesignClassTag,
    DesignEntityId, DesignRecipeReference, DesignRelaxedGuidText, DesignSecondaryIdentity, Located,
    MaybeRecordedValue, NonEmptyVec, RecordedValue, ReferenceRun, IDENTITY_MATRIX,
};
use cadmpeg_ir::math::{Point3, Vector3};
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::num::NonZeroU32;

/// Boolean result operation stored by an Extrude parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeOperation {
    /// Union the swept volume with the selected bodies.
    Join,
    /// Subtract the swept volume from the selected bodies.
    Cut,
    /// Retain the intersection of the swept volume and selected bodies.
    Intersect,
    /// Create an independent body.
    NewBody,
}

/// Decoded Extrude travel form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeExtent {
    /// Travel a signed fixed distance on the first side of the profile.
    OneSidedDistance,
    /// Travel on the first side until reaching a selected face or shape.
    OneSidedToFace,
    /// Travel on both sides until each side reaches its selected face.
    TwoSidedToFaces,
    /// Travel independent fixed distances on both sides of the profile.
    TwoSidedDistance,
    /// Travel a fixed distance on the first side and to a selected face on the second side.
    TwoSidedDistanceToFace,
    /// Travel one fixed total distance symmetrically around the profile plane.
    SymmetricDistance,
    /// Travel symmetrically through all material on both sides of the profile plane.
    SymmetricThroughAll,
    /// Travel on the first side until the next material region is exited.
    OneSidedThroughNext,
    /// Travel on the first side through all material.
    OneSidedThroughAll,
}

/// Starting support selected by the fixed Extrude prologue enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeStart {
    /// Start on the selected sketch's plane.
    ProfilePlane,
    /// Start on a parallel offset from the selected sketch's plane.
    OffsetProfilePlane,
    /// Start on a selected face.
    FromFace,
}

/// Indexed-record prefix preceding a reference-aware Extrude prologue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignExtrudePrologueReferenceWire",
    into = "DesignExtrudePrologueReferenceWire"
)]
pub struct DesignExtrudePrologueReference {
    /// Referenced Design record.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    pub trailing_zero_count: u8,
    /// Byte offset of the optional marker 1 before the operation.
    pub operation_prefix_marker_offset: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct DesignExtrudePrologueReferenceWire {
    /// Referenced Design record.
    record_index: u32,
    /// Byte offset of `record_index`.
    record_index_offset: u64,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    trailing_zero_count: u8,
    /// Optional marker byte between the zero run and the operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation_prefix_marker: Option<u8>,
    /// Byte offset of `operation_prefix_marker` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation_prefix_marker_offset: Option<u64>,
}

impl TryFrom<DesignExtrudePrologueReferenceWire> for DesignExtrudePrologueReference {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            record_index_offset: wire.record_index_offset,
            trailing_zero_count: wire.trailing_zero_count,
            operation_prefix_marker_offset: match (wire.operation_prefix_marker, wire.operation_prefix_marker_offset) {
                (None, None) => None,
                (Some(1), Some(offset)) => Some(offset),
                _ => return Err("operation_prefix_marker must be 1 and occur with operation_prefix_marker_offset".into()),
            },
        })
    }
}

impl From<DesignExtrudePrologueReference> for DesignExtrudePrologueReferenceWire {
    fn from(record: DesignExtrudePrologueReference) -> Self {
        Self {
            record_index: record.record_index,
            record_index_offset: record.record_index_offset,
            trailing_zero_count: record.trailing_zero_count,
            operation_prefix_marker: record.operation_prefix_marker_offset.map(|_| 1),
            operation_prefix_marker_offset: record.operation_prefix_marker_offset,
        }
    }
}

/// Scope-reference ordinal repeated before a whole-body Extrude target extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignExtrudeTargetOrdinal {
    /// Zero-based ordinal in the enclosing scope reference table.
    pub scope_reference_ordinal: u32,
    /// Byte offset of `scope_reference_ordinal`.
    pub scope_reference_ordinal_offset: u64,
}

/// Fixed fields preceding an Extrude parameter scope's reference table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignExtrudePrologueWire",
    into = "DesignExtrudePrologueWire"
)]
pub enum DesignExtrudePrologue {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Byte offset of the fixed zero prefix when its marker is present.
        prefix_zero_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Byte offset of the fixed extent-kind value `2`.
        #[serde(alias = "extent_discriminator_offset")]
        extent_kind_offset: u64,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of the geometry-kind integer.
        solid_operation_offset: u64,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Byte offset of the optional marker 1 before the operation.
        operation_prefix_marker_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "layout")]
enum DesignExtrudePrologueWire {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Value of the nullable prefix field when its marker is present.
        prefix_value: Option<u32>,
        /// Byte offset of `prefix_value` when present.
        prefix_value_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw extent-kind value (`2 = one-sided distance`).
        #[serde(alias = "extent_discriminator")]
        extent_kind: u32,
        /// Byte offset of `extent_kind`.
        #[serde(alias = "extent_discriminator_offset")]
        extent_kind_offset: u64,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Raw geometry-kind discriminator (`0 = sheet`, `1 = solid`).
        geometry_kind: u32,
        /// Byte offset of `geometry_kind`.
        geometry_kind_offset: u64,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Optional marker immediately before the operation fields.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker: Option<u8>,
        /// Byte offset of `operation_prefix_marker` when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker_offset: Option<u64>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: u64,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        #[serde(default)]
        side_extent_discriminator_offsets: [u64; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        #[serde(alias = "extent_discriminator_offsets")]
        direction_face_extend_offsets: [u64; 2],
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: u64,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: u64,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: u64,
    },
}

impl TryFrom<DesignExtrudePrologueWire> for DesignExtrudePrologue {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignExtrudePrologueWire::LegacyDistance { prefix_value, prefix_value_offset, operation, operation_offset, extent_kind, extent_kind_offset, direction_reversed, direction_reversed_offset, geometry_kind, geometry_kind_offset } => {
                let prefix_zero_offset = match (prefix_value, prefix_value_offset) {
                    (None, None) => None,
                    (Some(0), Some(offset)) => Some(offset),
                    _ => return Err("prefix_value must be zero and occur with prefix_value_offset".into()),
                };
                if extent_kind != 2 {
                    return Err("extent_kind must be 2 in a legacy distance prologue".into());
                }
                let solid_operation = match geometry_kind {
                    0 => false,
                    1 => true,
                    _ => return Err("geometry_kind must be 0 or 1".into()),
                };
                Self::LegacyDistance { prefix_zero_offset, operation, operation_offset, extent_kind_offset, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset: geometry_kind_offset }
            },
            DesignExtrudePrologueWire::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ReferenceAware { reference, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, first_side_target_ordinal, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologueWire::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::ShiftedReferenceAware { operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
            DesignExtrudePrologueWire::LegacyShifted { operation_prefix_marker, operation_prefix_marker_offset, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset } => Self::LegacyShifted { operation_prefix_marker_offset: match (operation_prefix_marker, operation_prefix_marker_offset) {
                (None, None) => None,
                (Some(1), Some(offset)) => Some(offset),
                _ => return Err("operation_prefix_marker must be 1 and occur with operation_prefix_marker_offset".into()),
            }, operation, operation_offset, direction_face_extend_values, side_extent_discriminators, side_extent_discriminator_offsets, extent, direction_face_extend_offsets, direction_reversed, direction_reversed_offset, solid_operation, solid_operation_offset, start, start_offset },
        })
    }
}

impl From<DesignExtrudePrologue> for DesignExtrudePrologueWire {
    fn from(record: DesignExtrudePrologue) -> Self {
        match record {
            DesignExtrudePrologue::LegacyDistance {
                prefix_zero_offset,
                operation,
                operation_offset,
                extent_kind_offset,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
            } => Self::LegacyDistance {
                prefix_value: prefix_zero_offset.map(|_| 0),
                prefix_value_offset: prefix_zero_offset,
                operation,
                operation_offset,
                extent_kind: 2,
                extent_kind_offset,
                direction_reversed,
                direction_reversed_offset,
                geometry_kind: u32::from(solid_operation),
                geometry_kind_offset: solid_operation_offset,
            },
            DesignExtrudePrologue::ReferenceAware {
                reference,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            } => Self::ReferenceAware {
                reference,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            },
            DesignExtrudePrologue::ShiftedReferenceAware {
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            } => Self::ShiftedReferenceAware {
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            },
            DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            } => Self::LegacyShifted {
                operation_prefix_marker: operation_prefix_marker_offset.map(|_| 1),
                operation_prefix_marker_offset,
                operation,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed,
                direction_reversed_offset,
                solid_operation,
                solid_operation_offset,
                start,
                start_offset,
            },
        }
    }
}

impl DesignExtrudePrologue {
    /// Boolean result operation.
    pub fn operation(self) -> DesignExtrudeOperation {
        match self {
            Self::LegacyDistance { operation, .. }
            | Self::ReferenceAware { operation, .. }
            | Self::ShiftedReferenceAware { operation, .. }
            | Self::LegacyShifted { operation, .. } => operation,
        }
    }

    /// Decoded extent form.
    pub fn extent(self) -> Option<DesignExtrudeExtent> {
        match self {
            Self::LegacyDistance { .. } => Some(DesignExtrudeExtent::OneSidedDistance),
            Self::ReferenceAware { extent, .. } => Some(extent),
            Self::ShiftedReferenceAware { extent, .. } => Some(extent),
            Self::LegacyShifted { extent, .. } => extent,
        }
    }

    /// Direction-reversal state.
    pub fn direction_reversed(self) -> bool {
        match self {
            Self::LegacyDistance {
                direction_reversed, ..
            }
            | Self::ReferenceAware {
                direction_reversed, ..
            }
            | Self::ShiftedReferenceAware {
                direction_reversed, ..
            }
            | Self::LegacyShifted {
                direction_reversed, ..
            } => direction_reversed,
        }
    }

    /// Whether the operation creates solid rather than sheet geometry.
    pub fn solid_operation(self) -> bool {
        match self {
            Self::LegacyDistance {
                solid_operation, ..
            }
            | Self::ReferenceAware {
                solid_operation, ..
            }
            | Self::ShiftedReferenceAware {
                solid_operation, ..
            }
            | Self::LegacyShifted {
                solid_operation, ..
            } => solid_operation,
        }
    }

    /// Starting support.
    pub fn start(self) -> DesignExtrudeStart {
        match self {
            Self::LegacyDistance { .. } => DesignExtrudeStart::ProfilePlane,
            Self::ReferenceAware { start, .. }
            | Self::ShiftedReferenceAware { start, .. }
            | Self::LegacyShifted { start, .. } => start,
        }
    }
}

/// Driving-dimension mode stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignCoilExtent {
    /// Revolution count and total height are independent.
    RevolutionsHeight,
    /// Revolution count and pitch are independent.
    RevolutionsPitch,
    /// Total height and pitch are independent.
    HeightPitch,
    /// Revolution count and radial pitch define a planar spiral.
    Spiral,
}

/// Generated section family stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignCoilSection {
    /// Circular section.
    Circular,
    /// Square section.
    Square,
    /// Triangular section pointing away from the axis.
    ExternalTriangle,
    /// Triangular section pointing toward the axis.
    InternalTriangle,
}

/// Radial section placement stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignCoilSectionPlacement {
    /// Section inside the reference trajectory.
    Inside,
    /// Section centered on the reference trajectory.
    Center,
    /// Section outside the reference trajectory.
    Outside,
}

/// Construction-recipe families admitted by a face selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ConstructionRecipeKind", into = "ConstructionRecipeKind")]
pub enum DesignFaceRecipeKind {
    Face,
    BoundedFace,
}

impl TryFrom<ConstructionRecipeKind> for DesignFaceRecipeKind {
    type Error = String;

    fn try_from(kind: ConstructionRecipeKind) -> Result<Self, Self::Error> {
        match kind {
            ConstructionRecipeKind::Face => Ok(Self::Face),
            ConstructionRecipeKind::BoundedFace => Ok(Self::BoundedFace),
            ConstructionRecipeKind::Body
            | ConstructionRecipeKind::Edge
            | ConstructionRecipeKind::Vertex => {
                Err("recipe_kind must be face or bounded_face".into())
            }
        }
    }
}

impl From<DesignFaceRecipeKind> for ConstructionRecipeKind {
    fn from(kind: DesignFaceRecipeKind) -> Self {
        match kind {
            DesignFaceRecipeKind::Face => Self::Face,
            DesignFaceRecipeKind::BoundedFace => Self::BoundedFace,
        }
    }
}

fn deserialize_coil_secondary_identity<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DesignSecondaryIdentity<u64>>, D::Error> {
    #[derive(Deserialize)]
    struct Wire {
        secondary_identity: Option<u64>,
        curve_secondary_identity: Option<u64>,
    }
    let wire = Wire::deserialize(deserializer)?;
    DesignSecondaryIdentity::from_wire(wire.secondary_identity, wire.curve_secondary_identity)
        .map_err(serde::de::Error::custom)
}

fn deserialize_coil_recipe_design<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ConstructionRecipeDesign<String>>, D::Error> {
    #[derive(Deserialize)]
    struct Wire {
        design_id: Option<String>,
        design_selector: Option<ConstructionRecipeSelector>,
    }
    let wire = Wire::deserialize(deserializer)?;
    match (wire.design_id, wire.design_selector) {
        (Some(id), selector) => Ok(Some(ConstructionRecipeDesign { id, selector })),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(serde::de::Error::custom(
            "design_selector requires design_id",
        )),
    }
}

/// Selection carrier used by a compact Coil placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignCoilSelection {
    /// Nested entity-selection frame with one or two persistent identities.
    Persistent {
        /// Asset UUID qualifying the persistent selection namespace.
        asset_id: DesignRelaxedGuidText,
        /// Context UUID qualifying the persistent selection namespace.
        context_id: DesignRelaxedGuidText,
        /// Indexed nested record carrying the persistent identity pair.
        identity_record_index: u32,
        /// First persistent identity value.
        primary_identity: u64,
        /// Secondary identity and any dependent curve identity.
        #[serde(flatten, deserialize_with = "deserialize_coil_secondary_identity")]
        secondary: Option<DesignSecondaryIdentity<u64>>,
    },
    /// Face construction recipe carried by a placement selection frame.
    FaceRecipe {
        /// Asset UUID qualifying the recipe selection namespace.
        asset_id: DesignRelaxedGuidText,
        /// Context UUID qualifying the recipe selection namespace.
        context_id: DesignRelaxedGuidText,
        /// Indexed record containing the face recipe.
        recipe_record_index: u32,
        /// Byte offset of the face recipe record header.
        recipe_record_byte_offset: u64,
        /// Native construction-recipe arena identity.
        recipe_id: String,
        /// Exact face-recipe family.
        recipe_kind: DesignFaceRecipeKind,
        /// Recipe Design identity and its optional selector.
        #[serde(flatten, deserialize_with = "deserialize_coil_recipe_design")]
        design: Option<ConstructionRecipeDesign<String>>,
    },
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignCoilPlacementWire", into = "DesignCoilPlacementWire")]
pub struct DesignCoilPlacement {
    /// First ordered placement-construction reference.
    pub selection_record_index: u32,
    /// Byte offset of the support selection frame header.
    pub selection_record_byte_offset: u64,
    /// Dynamic class tag of the support selection frame.
    pub selection_class_tag: DesignClassTag,
    /// Exact selection semantics carried by the first placement reference.
    pub selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    pub transform_record_index: u32,
    /// Byte offset of the frame carrier header.
    pub transform_record_byte_offset: u64,
    /// Dynamic class tag of the frame carrier.
    pub transform_class_tag: DesignClassTag,
    /// Explicit matrix and its byte offset; absent for the encoded identity form.
    pub explicit_transform: Option<Located<SketchPlacementMatrix>>,
}

impl DesignCoilPlacement {
    /// Row-major local-to-model matrix with translation in source centimetres.
    #[must_use]
    pub fn transform(&self) -> &SketchPlacementMatrix {
        self.explicit_transform
            .as_ref()
            .map_or(&SketchPlacementMatrix::IDENTITY, |matrix| &matrix.value)
    }
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCoilPlacementWire {
    /// First ordered placement-construction reference.
    selection_record_index: u32,
    /// Byte offset of the support selection frame header.
    selection_record_byte_offset: u64,
    /// Dynamic class tag of the support selection frame.
    selection_class_tag: String,
    /// Exact selection semantics carried by the first placement reference.
    selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    transform_record_index: u32,
    /// Byte offset of the frame carrier header.
    transform_record_byte_offset: u64,
    /// Dynamic class tag of the frame carrier.
    transform_class_tag: String,
    /// Row-major local-to-model rigid transform. Matrix values are in source
    /// centimetres for the translation column.
    transform: SketchPlacementMatrix,
    /// Byte offset of the matrix, or absent for the encoded identity form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
}

impl TryFrom<DesignCoilPlacementWire> for DesignCoilPlacement {
    type Error = String;
    fn try_from(wire: DesignCoilPlacementWire) -> Result<Self, Self::Error> {
        let explicit_transform = match wire.transform_offset {
            Some(offset) => Some(Located {
                value: wire.transform,
                offset,
            }),
            None => {
                if wire
                    .transform
                    .iter()
                    .flatten()
                    .zip(IDENTITY_MATRIX.iter().flatten())
                    .any(|(value, identity)| value.to_bits() != identity.to_bits())
                {
                    return Err("transform must be identity when transform_offset is absent".into());
                }
                None
            }
        };
        Ok(Self {
            selection_record_index: wire.selection_record_index,
            selection_record_byte_offset: wire.selection_record_byte_offset,
            selection_class_tag: wire.selection_class_tag.try_into()?,
            selection: wire.selection,
            transform_record_index: wire.transform_record_index,
            transform_record_byte_offset: wire.transform_record_byte_offset,
            transform_class_tag: wire.transform_class_tag.try_into()?,
            explicit_transform,
        })
    }
}

impl From<DesignCoilPlacement> for DesignCoilPlacementWire {
    fn from(record: DesignCoilPlacement) -> Self {
        let transform = *record.transform();
        Self {
            selection_record_index: record.selection_record_index,
            selection_record_byte_offset: record.selection_record_byte_offset,
            selection_class_tag: record.selection_class_tag.into(),
            selection: record.selection,
            transform_record_index: record.transform_record_index,
            transform_record_byte_offset: record.transform_record_byte_offset,
            transform_class_tag: record.transform_class_tag.into(),
            transform,
            transform_offset: record.explicit_transform.map(|matrix| matrix.offset),
        }
    }
}

/// Direct rigid placement carried by the long ten-reference Coil form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignCoilTransform {
    /// Row-major local-to-model rigid transform. Translation is in source
    /// centimetres.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the shifted-form source frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

/// One exact scalar carrier used by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFixedExtrudeScalar {
    /// Scalar value in source centimetres for a distance or radians for an angle.
    pub value: f64,
    /// Referenced record carrying the scalar.
    pub record_index: u32,
    /// Byte offset of the scalar.
    pub value_offset: u64,
}

/// Exact carrier of an Extrude's one-sided distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "carrier", content = "scalar", rename_all = "snake_case")]
pub enum DesignFixedExtrudeDistance {
    /// Signed distance in an owner-local scalar lane.
    FixedScalar(DesignFixedExtrudeScalar),
    /// Positive magnitude in an owned distance-construction frame.
    DistanceConstruction(DesignFixedExtrudeScalar),
}

/// Exact fixed scalar lanes carried by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFixedExtrudeParameters {
    /// One-sided distance carrier in source centimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub along_distance: Option<DesignFixedExtrudeDistance>,
    /// Taper-angle lane in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper_angle: Option<DesignFixedExtrudeScalar>,
}

/// Exact fixed scalar lanes carried by a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFixedFilletParameters {
    /// Radius laws in scalar-lane order.
    pub groups: Vec<DesignFixedFilletGroup>,
}

/// One fillet radius law and its optional tangency weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFixedFilletGroupWire",
    into = "DesignFixedFilletGroupWire"
)]
pub struct DesignFixedFilletGroup {
    tangency_weight: Option<DesignFixedFilletScalar>,
    law: DesignFixedFilletLaw,
}

impl DesignFixedFilletGroup {
    /// Admit a fillet radius law and optional tangency weight.
    pub fn try_new(
        tangency_weight: Option<DesignFixedFilletScalar>,
        law: DesignFixedFilletLaw,
    ) -> Result<Self, String> {
        if tangency_weight
            .as_ref()
            .is_some_and(|weight| DesignPositiveScalar::new(weight.value).is_none())
        {
            return Err("tangency_weight must be positive and finite".into());
        }
        if !law
            .radii()
            .all(|radius| radius.value.is_finite() && radius.value >= 0.0)
        {
            return Err("radii must be finite and non-negative".into());
        }
        if !law.radii().any(|radius| radius.value > 0.0) {
            return Err("radii must contain a positive radius".into());
        }
        if !law
            .intermediate()
            .iter()
            .all(|row| row.parameter.value.is_finite() && (0.0..1.0).contains(&row.parameter.value))
        {
            return Err("intermediate_parameters must be finite and in [0, 1)".into());
        }
        if !law
            .intermediate()
            .windows(2)
            .all(|rows| rows[0].parameter.value < rows[1].parameter.value)
        {
            return Err("intermediate_parameters must be strictly increasing".into());
        }
        Ok(Self {
            tangency_weight,
            law,
        })
    }

    /// The admitted radius law.
    pub fn law(&self) -> &DesignFixedFilletLaw {
        &self.law
    }

    /// The optional positive tangency weight.
    pub fn tangency_weight(&self) -> Option<&DesignFixedFilletScalar> {
        self.tangency_weight.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DesignFixedFilletLaw {
    Constant(DesignFixedFilletScalar),
    Variable {
        start: DesignFixedFilletScalar,
        end: DesignFixedFilletScalar,
        intermediate: Vec<DesignFixedFilletIntermediate>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignFixedFilletIntermediate {
    pub radius: DesignFixedFilletScalar,
    pub parameter: DesignFixedFilletScalar,
}

impl DesignFixedFilletLaw {
    pub(crate) fn radii(&self) -> impl Iterator<Item = &DesignFixedFilletScalar> {
        let (first, second, intermediate) = match self {
            Self::Constant(radius) => (radius, None, &[][..]),
            Self::Variable {
                start,
                end,
                intermediate,
            } => (start, Some(end), intermediate.as_slice()),
        };
        std::iter::once(first)
            .chain(second)
            .chain(intermediate.iter().map(|row| &row.radius))
    }

    pub(crate) fn intermediate(&self) -> &[DesignFixedFilletIntermediate] {
        match self {
            Self::Constant(_) => &[],
            Self::Variable { intermediate, .. } => intermediate,
        }
    }
}

/// One Fillet radius law carried by fixed scalar lanes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignFixedFilletGroupWire {
    /// Optional explicit dimensionless tangency-weight lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangency_weight: Option<DesignFixedFilletScalar>,
    /// One constant radius, or endpoint radii followed by intermediate radii,
    /// in source centimetres.
    radii: Vec<f64>,
    /// Referenced radius scalar records in semantic radius order.
    radius_record_indexes: Vec<u32>,
    /// Byte offsets of the radius scalars in semantic radius order.
    radius_offsets: Vec<u64>,
    /// Normalized edge-chain positions paired with the intermediate radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameters: Vec<f64>,
    /// Referenced intermediate-position scalar records in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameter_record_indexes: Vec<u32>,
    /// Byte offsets of intermediate-position scalars in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameter_offsets: Vec<u64>,
}

impl TryFrom<DesignFixedFilletGroupWire> for DesignFixedFilletGroup {
    type Error = String;

    fn try_from(wire: DesignFixedFilletGroupWire) -> Result<Self, Self::Error> {
        if wire.radii.len() != wire.radius_record_indexes.len()
            || wire.radii.len() != wire.radius_offsets.len()
        {
            return Err(
                "radii, radius_record_indexes, and radius_offsets must have equal lengths".into(),
            );
        }
        if wire.intermediate_parameters.len() != wire.intermediate_parameter_record_indexes.len()
            || wire.intermediate_parameters.len() != wire.intermediate_parameter_offsets.len()
        {
            return Err("intermediate_parameters, intermediate_parameter_record_indexes, and intermediate_parameter_offsets must have equal lengths".into());
        }
        let mut radii = wire
            .radii
            .into_iter()
            .zip(wire.radius_record_indexes)
            .zip(wire.radius_offsets)
            .map(
                |((value, record_index), value_offset)| DesignFixedFilletScalar {
                    value,
                    record_index,
                    value_offset,
                },
            );
        let start = radii
            .next()
            .ok_or("radii requires a constant radius or two endpoints")?;
        let law = match radii.next() {
            None if wire.intermediate_parameters.is_empty() => {
                DesignFixedFilletLaw::Constant(start)
            }
            None => return Err("intermediate_parameters requires two endpoint radii".into()),
            Some(end) => {
                if radii.len() != wire.intermediate_parameters.len() {
                    return Err("radii must contain two endpoints and one radius per intermediate_parameters entry".into());
                }
                let parameters = wire
                    .intermediate_parameters
                    .into_iter()
                    .zip(wire.intermediate_parameter_record_indexes)
                    .zip(wire.intermediate_parameter_offsets)
                    .map(
                        |((value, record_index), value_offset)| DesignFixedFilletScalar {
                            value,
                            record_index,
                            value_offset,
                        },
                    );
                DesignFixedFilletLaw::Variable {
                    start,
                    end,
                    intermediate: radii
                        .zip(parameters)
                        .map(|(radius, parameter)| DesignFixedFilletIntermediate {
                            radius,
                            parameter,
                        })
                        .collect(),
                }
            }
        };
        Self::try_new(wire.tangency_weight, law)
    }
}

impl From<DesignFixedFilletGroup> for DesignFixedFilletGroupWire {
    fn from(group: DesignFixedFilletGroup) -> Self {
        Self {
            tangency_weight: group.tangency_weight,
            radii: group.law.radii().map(|scalar| scalar.value).collect(),
            radius_record_indexes: group
                .law
                .radii()
                .map(|scalar| scalar.record_index)
                .collect(),
            radius_offsets: group
                .law
                .radii()
                .map(|scalar| scalar.value_offset)
                .collect(),
            intermediate_parameters: group
                .law
                .intermediate()
                .iter()
                .map(|row| row.parameter.value)
                .collect(),
            intermediate_parameter_record_indexes: group
                .law
                .intermediate()
                .iter()
                .map(|row| row.parameter.record_index)
                .collect(),
            intermediate_parameter_offsets: group
                .law
                .intermediate()
                .iter()
                .map(|row| row.parameter.value_offset)
                .collect(),
        }
    }
}

/// One fixed fillet scalar and its source record and value location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFixedFilletScalar {
    /// Radius, normalized position, or tangency weight.
    pub value: f64,
    /// Referenced scalar record.
    pub record_index: u32,
    /// Byte offset of the scalar.
    pub value_offset: u64,
}

/// Exact construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignCircularPatternConstruction {
    /// Positive total instance count, including the seed.
    pub count: u32,
    /// Referenced compact count-parameter owner.
    pub count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub count_offset: u64,
    /// Positive angular span in radians.
    pub angle: f64,
    /// Referenced total-angle scalar.
    pub angle_record_index: u32,
    /// Byte offset of the total-angle scalar.
    pub angle_offset: u64,
    /// Serialized axis construction and its resolved placement.
    pub axis: DesignCircularPatternAxis,
    /// Referenced axis record.
    pub axis_record_index: u32,
    /// Referenced persistent selection operand.
    pub selection_record_index: u32,
}

/// Proven origin and unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignAxis {
    pub origin: Point3,
    pub direction: Vector3,
}

/// Proven origin and unit normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignPlane {
    pub origin: Point3,
    pub normal: Vector3,
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCircularPatternAxisWire",
    into = "DesignCircularPatternAxisWire"
)]
pub enum DesignCircularPatternAxis {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [f64; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Unit axis direction derived from the serialized displacement.
        direction: [f64; 3],
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through wrappers of one persistent historical topology identity.
    HistoricalEdge {
        /// Referenced wrappers and the offsets of their shared identity.
        wrappers: Vec<DesignPatternAxisWrapper>,
        /// Persistent ASM identity shared by the wrappers.
        persistent_identity: u64,
        /// Resolved model-space axis, when exact.
        resolved: Option<DesignAxis>,
    },
}

/// One historical axis wrapper and the location of its persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignPatternAxisWrapper {
    pub record_index: u32,
    pub identity_offset: u64,
}

/// Axis construction carried by a fixed circular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignCircularPatternAxisWire {
    /// Axis coordinates stored directly in the Design record.
    Inline {
        /// Axis origin in source centimetres.
        origin: [f64; 3],
        /// Byte offset of the first origin coordinate.
        origin_offset: u64,
        /// Unit axis direction derived from the serialized displacement.
        direction: [f64; 3],
        /// Byte offset of the first direction coordinate.
        direction_offset: u64,
    },
    /// Axis selected through wrappers of one persistent historical topology identity.
    HistoricalEdge {
        /// Referenced Design wrapper records, in serialized order.
        wrapper_record_indices: Vec<u32>,
        /// Persistent ASM identities carried by the wrappers.
        persistent_identities: Vec<u64>,
        /// Identity byte offsets parallel to `wrapper_record_indices`.
        identity_offsets: Vec<u64>,
        /// Resolved model-space axis, when exact.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_origin: Option<Point3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_direction: Option<Vector3>,
    },
}

impl TryFrom<DesignCircularPatternAxisWire> for DesignCircularPatternAxis {
    type Error = String;

    fn try_from(wire: DesignCircularPatternAxisWire) -> Result<Self, Self::Error> {
        match wire {
            DesignCircularPatternAxisWire::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Ok(Self::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            }),
            DesignCircularPatternAxisWire::HistoricalEdge {
                wrapper_record_indices,
                persistent_identities,
                identity_offsets,
                resolved_origin,
                resolved_direction,
            } => {
                if wrapper_record_indices.len() != identity_offsets.len() {
                    return Err(
                        "wrapper_record_indices and identity_offsets must have equal lengths"
                            .into(),
                    );
                }
                let [persistent_identity] = persistent_identities.as_slice() else {
                    return Err("persistent_identities must contain one shared identity".into());
                };
                let resolved = match (resolved_origin, resolved_direction) {
                    (None, None) => None,
                    (Some(origin), Some(direction)) => Some(DesignAxis { origin, direction }),
                    _ => {
                        return Err(
                            "resolved_origin and resolved_direction must occur together".into()
                        )
                    }
                };
                Ok(Self::HistoricalEdge {
                    wrappers: wrapper_record_indices
                        .into_iter()
                        .zip(identity_offsets)
                        .map(|(record_index, identity_offset)| DesignPatternAxisWrapper {
                            record_index,
                            identity_offset,
                        })
                        .collect(),
                    persistent_identity: *persistent_identity,
                    resolved,
                })
            }
        }
    }
}

impl From<DesignCircularPatternAxis> for DesignCircularPatternAxisWire {
    fn from(axis: DesignCircularPatternAxis) -> Self {
        match axis {
            DesignCircularPatternAxis::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            } => Self::Inline {
                origin,
                origin_offset,
                direction,
                direction_offset,
            },
            DesignCircularPatternAxis::HistoricalEdge {
                wrappers,
                persistent_identity,
                resolved,
            } => Self::HistoricalEdge {
                wrapper_record_indices: wrappers
                    .iter()
                    .map(|wrapper| wrapper.record_index)
                    .collect(),
                persistent_identities: vec![persistent_identity],
                identity_offsets: wrappers
                    .iter()
                    .map(|wrapper| wrapper.identity_offset)
                    .collect(),
                resolved_origin: resolved.map(|axis| axis.origin),
                resolved_direction: resolved.map(|axis| axis.direction),
            },
        }
    }
}

/// Ordered scalar lanes carried by a rectangular-pattern scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRectangularPatternConstructionWire",
    into = "DesignRectangularPatternConstructionWire"
)]
pub struct DesignRectangularPatternConstruction {
    /// Positive U-direction instance count, including the seed.
    u_count: NonZeroU32,
    /// Positive V-direction instance count, including the seed.
    v_count: NonZeroU32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<DesignRectangularPatternInstances>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DesignRectangularPatternConstructionWire {
    /// Positive U-direction instance count, including the seed.
    pub u_count: u32,
    /// Positive V-direction instance count, including the seed.
    pub v_count: u32,
    /// Signed U-direction seed-to-final-instance span in source centimetres.
    pub u_extent: f64,
    /// Signed V-direction seed-to-final-instance span in source centimetres.
    pub v_extent: f64,
    /// Parameter-owner records for U count, V count, U extent, and V extent.
    pub owner_record_indices: [u32; 4],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: [u64; 4],
    /// Exact serialized instance sequence when one pattern direction is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<DesignRectangularPatternInstances>,
}

impl TryFrom<DesignRectangularPatternConstructionWire> for DesignRectangularPatternConstruction {
    type Error = &'static str;
    fn try_from(wire: DesignRectangularPatternConstructionWire) -> Result<Self, Self::Error> {
        let u_count = NonZeroU32::new(wire.u_count).ok_or("u_count must be nonzero")?;
        let v_count = NonZeroU32::new(wire.v_count).ok_or("v_count must be nonzero")?;
        if u_count.get() == 1 && v_count.get() == 1 {
            return Err("u_count and v_count must not both be one");
        }
        if !wire.u_extent.is_finite() || (u_count.get() == 1) != (wire.u_extent == 0.0) {
            return Err("u_extent must be finite and zero exactly when u_count is one");
        }
        if !wire.v_extent.is_finite() || (v_count.get() == 1) != (wire.v_extent == 0.0) {
            return Err("v_extent must be finite and zero exactly when v_count is one");
        }
        Ok(Self {
            u_count,
            v_count,
            u_extent: wire.u_extent,
            v_extent: wire.v_extent,
            owner_record_indices: wire.owner_record_indices,
            value_offsets: wire.value_offsets,
            instances: wire.instances,
        })
    }
}
impl From<DesignRectangularPatternConstruction> for DesignRectangularPatternConstructionWire {
    fn from(value: DesignRectangularPatternConstruction) -> Self {
        Self {
            u_count: value.u_count.get(),
            v_count: value.v_count.get(),
            u_extent: value.u_extent,
            v_extent: value.v_extent,
            owner_record_indices: value.owner_record_indices,
            value_offsets: value.value_offsets,
            instances: value.instances,
        }
    }
}
impl DesignRectangularPatternConstruction {
    pub(crate) fn u_count(&self) -> u32 {
        self.u_count.get()
    }
    pub(crate) fn v_count(&self) -> u32 {
        self.v_count.get()
    }
    pub(crate) fn u_extent(&self) -> f64 {
        self.u_extent
    }
    pub(crate) fn v_extent(&self) -> f64 {
        self.v_extent
    }
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRectangularPatternInstancesWire",
    into = "DesignRectangularPatternInstancesWire"
)]
pub enum DesignRectangularPatternInstances {
    Bodies(Vec<DesignPatternInstance>),
    Components {
        component_guid: DesignRelaxedGuidText,
        seed: DesignPatternComponentInstance,
        generated: Vec<DesignPatternComponentInstance>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignPatternInstance {
    pub record_index: u32,
    pub transform: Located<SketchPlacementMatrix>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignPatternComponentInstance {
    pub instance: DesignPatternInstance,
    pub occurrence_guid: DesignRelaxedGuidText,
}

impl DesignRectangularPatternInstances {
    pub fn instance_count(&self) -> usize {
        match self {
            Self::Bodies(instances) => instances.len(),
            Self::Components { generated, .. } => generated.len() + 1,
        }
    }

    pub fn frames(&self) -> impl DoubleEndedIterator<Item = &DesignPatternInstance> {
        let (bodies, seed, generated): (
            &[DesignPatternInstance],
            Option<&DesignPatternInstance>,
            &[DesignPatternComponentInstance],
        ) = match self {
            Self::Bodies(instances) => (instances, None, &[]),
            Self::Components {
                seed, generated, ..
            } => (&[], Some(&seed.instance), generated),
        };
        bodies
            .iter()
            .chain(seed)
            .chain(generated.iter().map(|row| &row.instance))
    }
}

/// Serialized placements of one linearized rectangular-pattern instance run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignRectangularPatternInstancesWire {
    /// Seed record followed by the generated-instance records in pattern order.
    record_indices: Vec<u32>,
    /// Row-major local-to-model placements parallel to `record_indices`.
    transforms: Vec<SketchPlacementMatrix>,
    /// Byte offsets of the first transform scalar parallel to `record_indices`.
    transform_offsets: Vec<u64>,
    /// Component occurrences carried by this run when the pattern repeats a component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    component_occurrences: Option<DesignComponentPatternOccurrencesWire>,
}

/// Component seed and generated occurrences carried by a rectangular pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignComponentPatternOccurrencesWire {
    /// Reusable local component definition shared by every occurrence.
    component_guid: DesignRelaxedGuidText,
    /// Existing seed occurrence.
    seed_occurrence_guid: DesignRelaxedGuidText,
    /// Newly generated occurrences in pattern order after the seed.
    generated_occurrence_guids: Vec<DesignRelaxedGuidText>,
}

impl TryFrom<DesignRectangularPatternInstancesWire> for DesignRectangularPatternInstances {
    type Error = String;
    fn try_from(wire: DesignRectangularPatternInstancesWire) -> Result<Self, Self::Error> {
        if wire.record_indices.len() != wire.transforms.len()
            || wire.record_indices.len() != wire.transform_offsets.len()
        {
            return Err(
                "record_indices, transforms, and transform_offsets must have equal lengths".into(),
            );
        }
        let frames = wire
            .record_indices
            .into_iter()
            .zip(wire.transforms)
            .zip(wire.transform_offsets)
            .map(|((record_index, value), offset)| DesignPatternInstance {
                record_index,
                transform: Located { value, offset },
            });
        let Some(component) = wire.component_occurrences else {
            return Ok(Self::Bodies(frames.collect()));
        };
        let mut frames = frames;
        let seed = frames
            .next()
            .ok_or("component_occurrences requires a seed frame")?;
        if frames.len() != component.generated_occurrence_guids.len() {
            return Err(
                "generated_occurrence_guids must match the generated instance frames".into(),
            );
        }
        Ok(Self::Components {
            component_guid: component.component_guid,
            seed: DesignPatternComponentInstance {
                instance: seed,
                occurrence_guid: component.seed_occurrence_guid,
            },
            generated: frames
                .zip(component.generated_occurrence_guids)
                .map(
                    |(instance, occurrence_guid)| DesignPatternComponentInstance {
                        instance,
                        occurrence_guid,
                    },
                )
                .collect(),
        })
    }
}

impl From<DesignRectangularPatternInstances> for DesignRectangularPatternInstancesWire {
    fn from(instances: DesignRectangularPatternInstances) -> Self {
        let record_indices = instances.frames().map(|row| row.record_index).collect();
        let transforms = instances.frames().map(|row| row.transform.value).collect();
        let transform_offsets = instances.frames().map(|row| row.transform.offset).collect();
        let component_occurrences = match instances {
            DesignRectangularPatternInstances::Bodies(_) => None,
            DesignRectangularPatternInstances::Components {
                component_guid,
                seed,
                generated,
            } => Some(DesignComponentPatternOccurrencesWire {
                component_guid,
                seed_occurrence_guid: seed.occurrence_guid,
                generated_occurrence_guids: generated
                    .into_iter()
                    .map(|row| row.occurrence_guid)
                    .collect(),
            }),
        };
        Self {
            record_indices,
            transforms,
            transform_offsets,
            component_occurrences,
        }
    }
}

mod assembly;
pub use assembly::{
    DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyAxialOperandTarget,
    DesignAssemblyAxialSelectorIdentity, DesignAssemblyLegacyOperand, DesignAssemblyLegacyOperands,
    DesignAssemblyLegacySelection, DesignAssemblyLimitKind, DesignAssemblyLimits,
    DesignAssemblyLimitsWire, DesignAssemblyOperandFrame, DesignAssemblyOperandPath,
    DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier, DesignAssemblySolvedFrame,
    DesignQualifiedAssemblyOperand,
};

/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignComponentInsertConstructionWire",
    into = "DesignComponentInsertConstructionWire"
)]
pub struct DesignComponentInsertConstruction {
    /// Scope-owned relation record.
    pub relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    pub carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_identity: Option<u64>,
    /// Occurrence-role GUID joining the carrier to the external-reference table.
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
    pub neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    pub neutron_role_offset: u64,
    /// Explicit scope-local placement and its optional repeated carrier location.
    /// Absence is the encoded identity form.
    pub placement: Option<DesignComponentInsertMatrix>,
}

/// Scope-local matrix with an optional equal matrix in the grouped carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignComponentInsertMatrix {
    pub scope: Located<SketchPlacementMatrix>,
    pub carrier_offset: Option<u64>,
}

impl DesignComponentInsertConstruction {
    #[must_use]
    pub fn transform(&self) -> &SketchPlacementMatrix {
        self.placement
            .as_ref()
            .map_or(&SketchPlacementMatrix::IDENTITY, |matrix| {
                &matrix.scope.value
            })
    }

    #[must_use]
    pub fn transform_offset(&self) -> Option<u64> {
        self.placement.as_ref().map(|matrix| matrix.scope.offset)
    }

    #[must_use]
    pub fn carrier_transform_offset(&self) -> Option<u64> {
        self.placement
            .as_ref()
            .and_then(|matrix| matrix.carrier_offset)
    }
}

/// External occurrence and placement joined through a `Component Insert` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignComponentInsertConstructionWire {
    /// Scope-owned relation record.
    relation_record_index: u32,
    /// Grouped occurrence carrier named by the relation record.
    carrier_record_index: u32,
    /// Eight-byte occurrence identity carried by the scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    occurrence_identity: Option<u64>,
    /// Occurrence-role GUID joining the carrier to the external-reference table.
    /// The role also accepts a GUID prefix followed by an underscore and URN, beyond relaxed GUID text.
    neutron_role: String,
    /// Byte offset of the occurrence-role string payload.
    neutron_role_offset: u64,
    /// Row-major local occurrence transform in centimetres.
    transform: SketchPlacementMatrix,
    /// Byte offset of the first scope-local transform scalar. `None` is the
    /// stored identity form, which has no scalar block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
    /// Byte offset of the equal transform's first scalar in the grouped
    /// carrier; absent when the carrier stores no scalar block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carrier_transform_offset: Option<u64>,
}

impl TryFrom<DesignComponentInsertConstructionWire> for DesignComponentInsertConstruction {
    type Error = String;
    fn try_from(wire: DesignComponentInsertConstructionWire) -> Result<Self, Self::Error> {
        let placement = match (wire.transform_offset, wire.carrier_transform_offset) {
            (Some(offset), carrier_offset) => Some(DesignComponentInsertMatrix {
                scope: Located {
                    value: wire.transform,
                    offset,
                },
                carrier_offset,
            }),
            (None, None) => {
                if wire
                    .transform
                    .iter()
                    .flatten()
                    .zip(IDENTITY_MATRIX.iter().flatten())
                    .any(|(value, identity)| value.to_bits() != identity.to_bits())
                {
                    return Err("transform must be identity when transform_offset is absent".into());
                }
                None
            }
            (None, Some(_)) => {
                return Err("carrier_transform_offset requires transform_offset".into())
            }
        };
        Ok(Self {
            relation_record_index: wire.relation_record_index,
            carrier_record_index: wire.carrier_record_index,
            occurrence_identity: wire.occurrence_identity,
            neutron_role: wire.neutron_role,
            neutron_role_offset: wire.neutron_role_offset,
            placement,
        })
    }
}

impl From<DesignComponentInsertConstruction> for DesignComponentInsertConstructionWire {
    fn from(record: DesignComponentInsertConstruction) -> Self {
        let transform = *record.transform();
        let transform_offset = record.transform_offset();
        let carrier_transform_offset = record.carrier_transform_offset();
        Self {
            relation_record_index: record.relation_record_index,
            carrier_record_index: record.carrier_record_index,
            occurrence_identity: record.occurrence_identity,
            neutron_role: record.neutron_role,
            neutron_role_offset: record.neutron_role_offset,
            transform,
            transform_offset,
            carrier_transform_offset,
        }
    }
}

/// Local component occurrence joined through a `DerivedInstance` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignDerivedInstanceConstruction {
    /// Scope prologue record referenced by the fixed field at scope offset 22.
    pub reference_record_index: u32,
    /// Scope-owned class-310 relation record.
    pub relation_record_index: u32,
    /// Class-380 component-occurrence carrier named by the relation.
    pub carrier_record_index: u32,
    /// Component definition GUID carried by the joined occurrence.
    pub component_guid: DesignRelaxedGuidText,
    /// Placed occurrence GUID carried by the joined occurrence.
    pub occurrence_guid: DesignRelaxedGuidText,
    /// Row-major local-to-model placement in centimetres.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the first scope-local transform scalar.
    pub transform_offset: u64,
}

/// One exact local component-occurrence carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignComponentOccurrenceWire",
    into = "DesignComponentOccurrenceWire"
)]
pub struct DesignComponentOccurrence {
    /// Stable native record identity.
    pub id: String,
    /// Indexed-record class carrying this occurrence.
    pub class_tag: DesignClassTag,
    /// Indexed carrier record.
    pub record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    pub component_record_index: u64,
    /// Stable component-definition GUID.
    pub component_guid: DesignRelaxedGuidText,
    /// Stable placed-occurrence GUID.
    pub occurrence_guid: DesignRelaxedGuidText,
    /// Base occurrence or a placed occurrence with its ordinal and matrix.
    placement: DesignComponentOccurrencePlacement,
}

/// Local occurrence payload before checked frame admission.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignComponentOccurrenceDraft {
    /// Stable native record identity.
    pub id: String,
    /// Indexed-record class carrying this occurrence.
    pub class_tag: DesignClassTag,
    /// Indexed carrier record.
    pub record_index: u32,
    /// Byte offset of the indexed header.
    pub byte_offset: u64,
    /// Referenced component-definition record.
    pub component_record_index: u64,
    /// Stable component-definition GUID.
    pub component_guid: DesignRelaxedGuidText,
    /// Stable placed-occurrence GUID.
    pub occurrence_guid: DesignRelaxedGuidText,
    /// Base occurrence or a placed occurrence with its ordinal and matrix.
    pub placement: DesignComponentOccurrencePlacement,
}

/// Placement envelope of a local component occurrence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DesignComponentOccurrencePlacement {
    /// First occurrence, with no explicit matrix payload.
    Base,
    /// Explicit matrix and one-based occurrence ordinal.
    Explicit {
        ordinal: NonZeroU32,
        transform: SketchPlacementMatrix,
    },
}

impl DesignComponentOccurrence {
    /// Admit a local occurrence with representable GUID and placement offsets.
    pub fn try_new(draft: DesignComponentOccurrenceDraft) -> Result<Self, String> {
        let last_offset = match draft.placement {
            DesignComponentOccurrencePlacement::Base => 124,
            DesignComponentOccurrencePlacement::Explicit { .. } => 209,
        };
        draft
            .byte_offset
            .checked_add(last_offset)
            .ok_or("component occurrence offsets overflow byte_offset")?;
        Ok(Self {
            id: draft.id,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            byte_offset: draft.byte_offset,
            component_record_index: draft.component_record_index,
            component_guid: draft.component_guid,
            occurrence_guid: draft.occurrence_guid,
            placement: draft.placement,
        })
    }

    /// Indexed header byte offset.
    pub fn byte_offset(&self) -> u64 {
        self.byte_offset
    }

    /// Component GUID byte offset.
    pub fn component_guid_offset(&self) -> u64 {
        self.byte_offset + 48
    }

    /// Occurrence GUID byte offset.
    pub fn occurrence_guid_offset(&self) -> u64 {
        self.byte_offset + 124
    }

    /// Base or explicit local placement.
    pub fn placement(&self) -> &DesignComponentOccurrencePlacement {
        &self.placement
    }

    #[must_use]
    pub fn occurrence_ordinal(&self) -> u32 {
        match self.placement {
            DesignComponentOccurrencePlacement::Base => 1,
            DesignComponentOccurrencePlacement::Explicit { ordinal, .. } => ordinal.get(),
        }
    }

    #[must_use]
    pub fn transform(&self) -> Option<Located<SketchPlacementMatrix>> {
        match self.placement {
            DesignComponentOccurrencePlacement::Base => None,
            DesignComponentOccurrencePlacement::Explicit { transform, .. } => Some(Located {
                value: transform,
                offset: self.byte_offset + 209,
            }),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DesignComponentOccurrenceWire {
    /// Stable native record identity.
    id: String,
    /// Indexed-record class carrying this occurrence.
    class_tag: String,
    /// Indexed carrier record.
    record_index: u32,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Referenced component-definition record.
    component_record_index: u64,
    /// Stable component-definition GUID.
    component_guid: DesignRelaxedGuidText,
    /// Byte offset of the component GUID payload.
    component_guid_offset: u64,
    /// Stable placed-occurrence GUID.
    occurrence_guid: DesignRelaxedGuidText,
    /// Byte offset of the occurrence GUID payload.
    occurrence_guid_offset: u64,
    /// One-based occurrence ordinal within the component definition.
    occurrence_ordinal: u32,
    /// Explicit local-to-model placement for placed occurrences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the explicit placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
}

impl From<DesignComponentOccurrence> for DesignComponentOccurrenceWire {
    fn from(value: DesignComponentOccurrence) -> Self {
        let occurrence_ordinal = value.occurrence_ordinal();
        let component_guid_offset = value.component_guid_offset();
        let occurrence_guid_offset = value.occurrence_guid_offset();
        let transform = value.transform();
        Self {
            id: value.id,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            component_guid_offset,
            occurrence_guid: value.occurrence_guid,
            occurrence_guid_offset,
            occurrence_ordinal,
            transform: transform.map(|frame| frame.value),
            transform_offset: transform.map(|frame| frame.offset),
        }
    }
}

impl TryFrom<DesignComponentOccurrenceWire> for DesignComponentOccurrence {
    type Error = String;
    fn try_from(value: DesignComponentOccurrenceWire) -> Result<Self, Self::Error> {
        let transform = Located::from_wire(value.transform, value.transform_offset, "transform")?;
        let placement = match (value.occurrence_ordinal, transform) {
            (1, None) => DesignComponentOccurrencePlacement::Base,
            (ordinal, Some(transform)) => DesignComponentOccurrencePlacement::Explicit {
                ordinal: NonZeroU32::new(ordinal).ok_or("occurrence_ordinal must be nonzero")?,
                transform: transform.value,
            },
            (_, None) => return Err("occurrence_ordinal must be 1 when transform is absent".into()),
        };
        let record = Self::try_new(DesignComponentOccurrenceDraft {
            id: value.id,
            class_tag: value.class_tag.try_into()?,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            component_record_index: value.component_record_index,
            component_guid: value.component_guid,
            occurrence_guid: value.occurrence_guid,
            placement,
        })?;
        if value.component_guid_offset != record.component_guid_offset() {
            return Err("component_guid_offset disagrees with byte_offset".into());
        }
        if value.occurrence_guid_offset != record.occurrence_guid_offset() {
            return Err("occurrence_guid_offset disagrees with byte_offset".into());
        }
        if value.transform_offset != record.transform().map(|transform| transform.offset) {
            return Err("transform_offset disagrees with byte_offset".into());
        }
        Ok(record)
    }
}

/// Legacy component copy/paste construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignCopyPasteComponentOperation {
    /// Scope-owned relation record.
    pub relation_record_index: u32,
    /// Existing source occurrence carrier.
    pub source_occurrence_record_index: u32,
    /// Newly copied occurrence carrier.
    pub copied_occurrence_record_index: u32,
    /// Reusable component definition shared by source and copy.
    pub component_guid: DesignRelaxedGuidText,
    /// Existing source occurrence identity.
    pub source_occurrence_guid: DesignRelaxedGuidText,
    /// Newly copied occurrence identity.
    pub copied_occurrence_guid: DesignRelaxedGuidText,
    /// Source placement embedded by the scope.
    pub source_transform: SketchPlacementMatrix,
    /// Byte offset of the source placement.
    pub source_transform_offset: u64,
    /// Copied placement embedded by both scope and occurrence carrier.
    pub copied_transform: SketchPlacementMatrix,
    /// Byte offset of the scope-local copied placement.
    pub copied_transform_offset: u64,
}

/// Exact construction carried by a Mirror scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMirrorConstructionWire",
    into = "DesignMirrorConstructionWire"
)]
pub struct DesignMirrorConstruction {
    /// Parameter-owner record carrying the fixed count two.
    pub count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    pub count_offset: u64,
    /// Positive model-space stitch tolerance in source centimetres.
    pub stitch_tolerance: f64,
    /// Byte offset of the evaluated stitch-tolerance scalar.
    pub stitch_tolerance_offset: u64,
    /// Owner-backed or inline scope-frame tolerance carrier.
    pub tolerance_source: DesignMirrorToleranceSource,
    /// Seed group selected by the source operation.
    pub seed_group_record_index: u32,
    /// Role-`0x5` mirror-plane group.
    pub plane_group_record_index: u32,
    /// Referenced seed feature scope when the seed is a complete feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_feature_scope_record_index: Option<Located<u32>>,
    /// Referenced `WorkPlane` scope, when the plane operand names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_scope_record_index: Option<Located<u32>>,
    /// Persistent entity-selection record used as the mirror plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_selection_record_index: Option<u32>,
    /// Proven selected-face mirror plane, when exact.
    pub plane: Option<DesignPlane>,
}

/// Native carrier of a Mirror stitch tolerance.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignMirrorToleranceSource {
    Owner { record_index: u32 },
    Scope(DesignMirrorScopeTolerance),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMirrorConstructionWire {
    /// Fixed instance count, including the seed.
    count: u32,
    /// Parameter-owner record carrying `count`.
    count_record_index: u32,
    /// Byte offset of the evaluated count scalar.
    count_offset: u64,
    /// Positive model-space stitch tolerance in source centimetres.
    stitch_tolerance: f64,
    /// Parameter-owner record carrying `stitch_tolerance`, when the source
    /// stores the scalar in a separate owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stitch_tolerance_record_index: Option<u32>,
    /// Byte offset of the evaluated stitch-tolerance scalar.
    stitch_tolerance_offset: u64,
    /// Inline scope-frame carrier used by the legacy Mirror envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stitch_tolerance_scope: Option<DesignMirrorScopeTolerance>,
    /// Seed group selected by the source operation.
    seed_group_record_index: u32,
    /// Role-`0x5` mirror-plane group.
    plane_group_record_index: u32,
    /// Referenced seed feature scope when the seed is a complete feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed_feature_scope_record_index: Option<u32>,
    /// Byte offset of the optional seed-feature reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed_feature_reference_offset: Option<u64>,
    /// Referenced `WorkPlane` scope, when the plane operand names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_scope_record_index: Option<u32>,
    /// Byte offset of the optional `WorkPlane` reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_reference_offset: Option<u64>,
    /// Persistent entity-selection record used as the mirror plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_selection_record_index: Option<u32>,
    /// Proven selected-face mirror plane, when exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_origin: Option<Point3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane_normal: Option<Vector3>,
}

impl TryFrom<DesignMirrorConstructionWire> for DesignMirrorConstruction {
    type Error = String;
    fn try_from(wire: DesignMirrorConstructionWire) -> Result<Self, Self::Error> {
        if wire.count != 2 {
            return Err("count must be 2 for Mirror".into());
        }
        let tolerance_source = match (wire.stitch_tolerance_record_index, wire.stitch_tolerance_scope) {
            (Some(record_index), None) => DesignMirrorToleranceSource::Owner { record_index },
            (None, Some(scope)) => DesignMirrorToleranceSource::Scope(scope),
            _ => return Err("stitch_tolerance_record_index and stitch_tolerance_scope require exactly one carrier".into()),
        };
        Ok(Self {
            count_record_index: wire.count_record_index,
            count_offset: wire.count_offset,
            stitch_tolerance: wire.stitch_tolerance,
            stitch_tolerance_offset: wire.stitch_tolerance_offset,
            tolerance_source,
            seed_group_record_index: wire.seed_group_record_index,
            plane_group_record_index: wire.plane_group_record_index,
            seed_feature_scope_record_index: Located::from_wire(wire.seed_feature_scope_record_index, wire.seed_feature_reference_offset, "seed_feature_scope_record_index").map_err(|_| "seed_feature_scope_record_index and seed_feature_reference_offset must occur together")?,
            plane_scope_record_index: Located::from_wire(wire.plane_scope_record_index, wire.plane_reference_offset, "plane_scope_record_index").map_err(|_| "plane_scope_record_index and plane_reference_offset must occur together")?,
            plane_selection_record_index: wire.plane_selection_record_index,
            plane: match (wire.plane_origin, wire.plane_normal) {
                (None, None) => None,
                (Some(origin), Some(normal)) => Some(DesignPlane { origin, normal }),
                _ => return Err("plane_origin and plane_normal must occur together".into()),
            },
        })
    }
}

impl From<DesignMirrorConstruction> for DesignMirrorConstructionWire {
    fn from(record: DesignMirrorConstruction) -> Self {
        let (stitch_tolerance_record_index, stitch_tolerance_scope) = match record.tolerance_source
        {
            DesignMirrorToleranceSource::Owner { record_index } => (Some(record_index), None),
            DesignMirrorToleranceSource::Scope(scope) => (None, Some(scope)),
        };
        Self {
            count: 2,
            count_record_index: record.count_record_index,
            count_offset: record.count_offset,
            stitch_tolerance: record.stitch_tolerance,
            stitch_tolerance_record_index,
            stitch_tolerance_offset: record.stitch_tolerance_offset,
            stitch_tolerance_scope,
            seed_group_record_index: record.seed_group_record_index,
            plane_group_record_index: record.plane_group_record_index,
            seed_feature_scope_record_index: record
                .seed_feature_scope_record_index
                .map(|reference| reference.value),
            seed_feature_reference_offset: record
                .seed_feature_scope_record_index
                .map(|reference| reference.offset),
            plane_scope_record_index: record
                .plane_scope_record_index
                .map(|reference| reference.value),
            plane_reference_offset: record
                .plane_scope_record_index
                .map(|reference| reference.offset),
            plane_selection_record_index: record.plane_selection_record_index,
            plane_origin: record.plane.map(|plane| plane.origin),
            plane_normal: record.plane.map(|plane| plane.normal),
        }
    }
}

/// Exact inline carrier for a legacy Mirror stitch tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMirrorScopeToleranceWire",
    into = "DesignMirrorScopeToleranceWire"
)]
pub struct DesignMirrorScopeTolerance {
    /// Fixed scalar-lane marker preceding the tolerance value.
    pub marker: DesignMirrorToleranceMarker,
    /// Byte offset of the first scalar-lane marker.
    pub marker_offset: u64,
    /// First marked reference in the scalar lane.
    pub first_reference: u32,
    /// Byte offset of the first marked reference.
    pub first_reference_offset: u64,
    /// Second marked reference in the scalar lane.
    pub second_reference: u32,
    /// Byte offset of the second marked reference.
    pub second_reference_offset: u64,
}

/// Scalar-lane marker and its required repeated location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignMirrorToleranceMarker {
    Single61,
    Repeated89(u64),
    Repeated94(u64),
    Repeated100(u64),
}

impl DesignMirrorToleranceMarker {
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Single61 => 61,
            Self::Repeated89(_) => 89,
            Self::Repeated94(_) => 94,
            Self::Repeated100(_) => 100,
        }
    }

    #[must_use]
    pub fn repeated_offset(self) -> Option<u64> {
        match self {
            Self::Single61 => None,
            Self::Repeated89(offset) | Self::Repeated94(offset) | Self::Repeated100(offset) => {
                Some(offset)
            }
        }
    }
}

impl TryFrom<(u32, Option<u64>)> for DesignMirrorToleranceMarker {
    type Error = String;
    fn try_from((marker, offset): (u32, Option<u64>)) -> Result<Self, Self::Error> {
        match (marker, offset) {
            (61, None) => Ok(Self::Single61),
            (89, Some(offset)) => Ok(Self::Repeated89(offset)),
            (94, Some(offset)) => Ok(Self::Repeated94(offset)),
            (100, Some(offset)) => Ok(Self::Repeated100(offset)),
            _ => Err("marker and repeated_marker_offset must identify a single 61 or repeated 89, 94, or 100 lane".into()),
        }
    }
}

/// Exact inline carrier for a legacy Mirror stitch tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMirrorScopeToleranceWire {
    /// Fixed scalar-lane marker preceding the tolerance value.
    marker: u32,
    /// Byte offset of the first scalar-lane marker.
    marker_offset: u64,
    /// Byte offset of the repeated scalar-lane marker, when this generation
    /// carries the marker twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeated_marker_offset: Option<u64>,
    /// First marked reference in the scalar lane.
    first_reference: u32,
    /// Byte offset of the first marked reference.
    first_reference_offset: u64,
    /// Second marked reference in the scalar lane.
    second_reference: u32,
    /// Byte offset of the second marked reference.
    second_reference_offset: u64,
}

impl TryFrom<DesignMirrorScopeToleranceWire> for DesignMirrorScopeTolerance {
    type Error = String;
    fn try_from(wire: DesignMirrorScopeToleranceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            marker: DesignMirrorToleranceMarker::try_from((
                wire.marker,
                wire.repeated_marker_offset,
            ))?,
            marker_offset: wire.marker_offset,
            first_reference: wire.first_reference,
            first_reference_offset: wire.first_reference_offset,
            second_reference: wire.second_reference,
            second_reference_offset: wire.second_reference_offset,
        })
    }
}

impl From<DesignMirrorScopeTolerance> for DesignMirrorScopeToleranceWire {
    fn from(record: DesignMirrorScopeTolerance) -> Self {
        Self {
            marker: record.marker.code(),
            marker_offset: record.marker_offset,
            repeated_marker_offset: record.marker.repeated_offset(),
            first_reference: record.first_reference,
            first_reference_offset: record.first_reference_offset,
            second_reference: record.second_reference,
            second_reference_offset: record.second_reference_offset,
        }
    }
}

/// Exact fixed scalar lanes carried by a Chamfer scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignFixedChamferParameters {
    /// One equal setback distance applies to both incident faces.
    EqualDistance {
        /// Equal setback distance.
        distance: DesignFixedChamferDistance,
    },
    /// The two incident faces have independently oriented setback distances.
    TwoDistances {
        /// Setback on the first incident face.
        first: DesignFixedChamferDistance,
        /// Setback on the second incident face.
        second: DesignFixedChamferDistance,
    },
}

/// One fixed Chamfer distance lane and its source provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFixedChamferDistance {
    /// Positive distance in source centimetres.
    pub value: f64,
    /// Referenced scalar record.
    pub record_index: u32,
    /// Byte offset of the scalar.
    pub value_offset: u64,
}

/// Exact construction carried by a Revolve, Loft, or Sweep scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignPathFeatureConstruction {
    /// One-sided fixed-angle revolution result operation.
    Revolve(DesignRevolveConstruction),
    /// Loft result operation.
    Loft(DesignLoftConstruction),
    /// Sweep result operation and fixed dimension lanes.
    Sweep(DesignSweepConstruction),
    /// Generated-section Pipe result and fixed dimension lanes.
    Pipe(DesignPipeConstruction),
}

/// Fixed construction of a `Revolve` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRevolveConstructionWire",
    into = "DesignRevolveConstructionWire"
)]
pub struct DesignRevolveConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Positive angular travel in radians.
    pub angle: DesignPositiveScalar,
    /// Referenced angular-travel scalar record.
    pub angle_record_index: u32,
    /// Byte offset of the angular-travel scalar.
    pub angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opposite_angle: Option<Located<u32>>,
}

#[derive(Serialize, Deserialize)]
struct DesignRevolveConstructionWire {
    /// Boolean result operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Positive angular travel in radians.
    angle: f64,
    /// Referenced angular-travel scalar record.
    angle_record_index: u32,
    /// Byte offset of the angular-travel scalar.
    angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opposite_angle_record_index: Option<u32>,
    /// Byte offset of the opposite-side angle scalar, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opposite_angle_offset: Option<u64>,
}

impl From<DesignRevolveConstruction> for DesignRevolveConstructionWire {
    fn from(value: DesignRevolveConstruction) -> Self {
        Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle: value.angle.get(),
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle_record_index: value.opposite_angle.map(|located| located.value),
            opposite_angle_offset: value.opposite_angle.map(|located| located.offset),
        }
    }
}

impl TryFrom<DesignRevolveConstructionWire> for DesignRevolveConstruction {
    type Error = String;
    fn try_from(value: DesignRevolveConstructionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle: DesignPositiveScalar::new(value.angle)
                .ok_or("angle must be positive and finite")?,
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle: match (
                value.opposite_angle_record_index,
                value.opposite_angle_offset,
            ) {
                (None, None) => None,
                (Some(value), Some(offset)) => Some(Located { value, offset }),
                _ => {
                    return Err(
                        "opposite_angle_record_index and opposite_angle_offset must occur together"
                            .into(),
                    )
                }
            },
        })
    }
}

/// Fixed construction of a `Loft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignLoftConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
}

/// Fixed construction of a `Sweep` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSweepConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Six scalar values in `AlongDistance`, `AgainstDistance`,
    /// `AlongRailDistance`, `AgainstRailDistance`, `TwistAngle`, and `TaperAngle` order.
    pub values: [f64; 6],
    /// Referenced scalar records in lane order.
    pub record_indexes: [u32; 6],
    /// Byte offsets of the scalar values in lane order.
    pub value_offsets: [u64; 6],
}

/// Fixed construction of a `Pipe` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignPipeConstruction {
    /// Boolean result operation.
    pub operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Section-shape selector byte.
    pub section_shape: DesignPipeSectionShape,
    /// Byte offset of the section-shape selector.
    pub section_shape_offset: u64,
    /// Whether the generated section is filled.
    pub filled: bool,
    /// Byte offset of the filled-section flag.
    pub filled_offset: u64,
    /// Four scalar values in path-fraction, reverse-path-fraction,
    /// section-size, and section-thickness order.
    pub values: [f64; 4],
    /// Referenced scalar records in lane order.
    pub record_indexes: [u32; 4],
    /// Byte offsets of the scalar values in lane order.
    pub value_offsets: [u64; 4],
}

/// Serialized prologue form of a `Combine` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignCombineForm {
    /// Nine zero bytes followed by the operation at offset 20.
    Standard,
    /// Class-387 form with the operation at offset 21.
    Compact,
    /// Eighteen-zero reference form with the operation at offset 31.
    ExtendedReference,
}

/// Version identity carried by a cross-document reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignExternalVersion {
    pub property_key: Located<DesignRelaxedGuidText>,
    pub version_urn: Located<String>,
}

/// Cross-document persistent body identity carried by a `Combine` tool selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCombineExternalBodyIdentityWire",
    into = "DesignCombineExternalBodyIdentityWire"
)]
pub struct DesignCombineExternalBodyIdentity {
    /// Asset GUID of the enclosing body selector.
    selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    external_version: Option<DesignExternalVersion>,
    /// Retained u64 values around the fixed `u32 48` member in the selector tail.
    #[serde(default)]
    tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    tail_value_offsets: [u64; 2],
}

/// Wire fields for an external Combine body identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignCombineExternalBodyIdentityWire {
    /// Asset GUID of the enclosing body selector.
    pub selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    pub selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    pub selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    pub selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    pub occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    pub external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    pub external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    pub external_segment: u32,
    /// Byte offset of `external_segment`.
    pub external_segment_offset: u64,
    /// Asset GUID carried by the cross-document body reference.
    pub external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    pub external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    pub external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_property_key: Option<DesignRelaxedGuidText>,
    /// Byte offset of `external_property_key` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_version_urn_offset: Option<u64>,
    /// Retained u64 values around the fixed `u32 48` member in the selector tail.
    #[serde(default)]
    pub tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    pub tail_value_offsets: [u64; 2],
}

impl DesignCombineExternalBodyIdentity {
    pub(crate) fn selector_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    pub(crate) fn selector_asset_id_offset(&self) -> u64 {
        self.selector_asset_id_offset
    }
    pub(crate) fn selector_context_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_context_id
    }
    pub(crate) fn occurrence_reference(&self) -> u64 {
        self.occurrence_reference
    }
    pub(crate) fn external_body_reference(&self) -> u64 {
        self.external_body_reference
    }
    pub(crate) fn external_segment(&self) -> u32 {
        self.external_segment
    }
    pub(crate) fn external_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    pub(crate) fn external_link_name(&self) -> &str {
        &self.external_link_name
    }
    pub(crate) fn external_version(&self) -> Option<&DesignExternalVersion> {
        self.external_version.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn tail_values(&self) -> [u64; 2] {
        self.tail_values
    }
    #[cfg(test)]
    pub(crate) fn tail_value_offsets(&self) -> [u64; 2] {
        self.tail_value_offsets
    }
    #[cfg(test)]
    pub(crate) fn external_asset_id_offset(&self) -> u64 {
        self.external_asset_id_offset
    }
}

impl TryFrom<DesignCombineExternalBodyIdentityWire> for DesignCombineExternalBodyIdentity {
    type Error = String;
    fn try_from(wire: DesignCombineExternalBodyIdentityWire) -> Result<Self, Self::Error> {
        let external_version = match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
            (None, None, None, None) => None,
            (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
            _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
        };
        if wire.external_asset_id != wire.selector_asset_id {
            return Err("external_asset_id must match selector_asset_id".into());
        }
        if wire.occurrence_reference == 0 {
            return Err("occurrence_reference must be nonzero".into());
        }
        if wire.external_body_reference == 0 {
            return Err("external_body_reference must be nonzero".into());
        }
        if wire.external_link_name.is_empty() {
            return Err("external_link_name must not be empty".into());
        }
        let utf16_end = |offset: u64, text: &str| -> Option<u64> {
            offset.checked_add(
                u64::try_from(text.encode_utf16().count())
                    .ok()?
                    .checked_mul(2)?,
            )
        };
        let after_text = |offset: u64, text: &str, delta: u64| {
            utf16_end(offset, text).and_then(|end| end.checked_add(delta))
        };
        let prefix = (crate::layout::combine_external_selector_prefix::LEN + 4) as u64;
        if wire.selector_asset_id_offset < prefix {
            return Err("selector_asset_id_offset must follow the selector header".into());
        }
        for (field, actual, expected) in [
            (
                "selector_context_id_offset",
                wire.selector_context_id_offset,
                after_text(
                    wire.selector_asset_id_offset,
                    wire.selector_asset_id.as_str(),
                    4,
                ),
            ),
            (
                "occurrence_reference_offset",
                wire.occurrence_reference_offset,
                after_text(
                    wire.selector_context_id_offset,
                    wire.selector_context_id.as_str(),
                    13,
                ),
            ),
            (
                "external_body_reference_offset",
                wire.external_body_reference_offset,
                wire.occurrence_reference_offset.checked_add(15),
            ),
            (
                "external_segment_offset",
                wire.external_segment_offset,
                wire.external_body_reference_offset.checked_add(9),
            ),
            (
                "external_asset_id_offset",
                wire.external_asset_id_offset,
                wire.external_segment_offset.checked_add(8),
            ),
            (
                "external_link_name_offset",
                wire.external_link_name_offset,
                after_text(
                    wire.external_asset_id_offset,
                    wire.external_asset_id.as_str(),
                    5,
                ),
            ),
            (
                "tail_value_offsets[1]",
                wire.tail_value_offsets[1],
                wire.tail_value_offsets[0].checked_add(12),
            ),
        ] {
            if expected != Some(actual) {
                return Err(format!(
                    "{field} disagrees with the external identity offset chain"
                ));
            }
        }
        let tail_offset = match &external_version {
            None => after_text(wire.external_link_name_offset, &wire.external_link_name, 7),
            Some(version) => {
                if version.version_urn.value.is_empty() {
                    return Err("external_version_urn must not be empty".into());
                }
                if after_text(wire.external_link_name_offset, &wire.external_link_name, 5)
                    != Some(version.property_key.offset)
                {
                    return Err(
                        "external_property_key_offset disagrees with external_link_name".into(),
                    );
                }
                if after_text(
                    version.property_key.offset,
                    version.property_key.value.as_str(),
                    4,
                ) != Some(version.version_urn.offset)
                {
                    return Err(
                        "external_version_urn_offset disagrees with external_property_key".into(),
                    );
                }
                after_text(version.version_urn.offset, &version.version_urn.value, 6)
            }
        };
        if tail_offset != Some(wire.tail_value_offsets[0]) {
            return Err(
                "tail_value_offsets[0] disagrees with the external identity offset chain".into(),
            );
        }
        Ok(Self {
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_body_reference: wire.external_body_reference,
            external_body_reference_offset: wire.external_body_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version,
            tail_values: wire.tail_values,
            tail_value_offsets: wire.tail_value_offsets,
        })
    }
}

impl From<DesignCombineExternalBodyIdentity> for DesignCombineExternalBodyIdentityWire {
    fn from(record: DesignCombineExternalBodyIdentity) -> Self {
        Self {
            selector_asset_id: record.selector_asset_id.clone(),
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_body_reference: record.external_body_reference,
            external_body_reference_offset: record.external_body_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.selector_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name,
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.clone()),
            external_property_key_offset: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.offset),
            external_version_urn: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.clone()),
            external_version_urn_offset: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.offset),
            tail_values: record.tail_values,
            tail_value_offsets: record.tail_value_offsets,
        }
    }
}

/// One target or tool body selector owned by a `Combine` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignCombineBodySelection {
    /// Body-selection record index.
    pub record_index: u32,
    /// Complete external body identity when the selector crosses a document boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identity: Option<DesignCombineExternalBodyIdentity>,
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCombineOperationWire",
    into = "DesignCombineOperationWire"
)]
pub struct DesignCombineOperation {
    /// Serialized scope-prologue form.
    pub form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    pub operation: cadmpeg_ir::features::BooleanKind,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    pub keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    pub keep_tools_offset: u64,
    /// Boolean target body selector.
    pub target_record_index: u32,
    /// Boolean tool body selectors in source order.
    pub tools: DesignCombineTools,
}

/// Ordered nonempty tools of a Combine operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignCombineTools {
    pub first: DesignCombineBodySelection,
    pub additional: Vec<DesignCombineBodySelection>,
}

impl DesignCombineTools {
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &DesignCombineBodySelection> {
        std::iter::once(&self.first).chain(&self.additional)
    }
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCombineOperationWire {
    /// Serialized scope-prologue form.
    form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    #[serde(deserialize_with = "deserialize_combine_operation_kind")]
    operation: cadmpeg_ir::features::BooleanKind,
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    keep_tools_offset: u64,
    /// Boolean target body selector.
    target: DesignCombineBodySelection,
    /// Boolean tool body selectors in source order.
    tools: Vec<DesignCombineBodySelection>,
}

fn deserialize_combine_operation_kind<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<cadmpeg_ir::features::BooleanKind, D::Error> {
    cadmpeg_ir::features::BooleanKind::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format!("operation: {error}")))
}

impl TryFrom<DesignCombineOperationWire> for DesignCombineOperation {
    type Error = String;
    fn try_from(wire: DesignCombineOperationWire) -> Result<Self, Self::Error> {
        if wire.target.external_identity.is_some() {
            return Err("target.external_identity must be absent".into());
        }
        let mut tools = wire.tools.into_iter();
        let first = tools
            .next()
            .ok_or("tools must contain at least one body selection")?;
        Ok(Self {
            form: wire.form,
            operation: wire.operation,
            operation_offset: wire.operation_offset,
            keep_tools: wire.keep_tools,
            keep_tools_offset: wire.keep_tools_offset,
            target_record_index: wire.target.record_index,
            tools: DesignCombineTools {
                first,
                additional: tools.collect(),
            },
        })
    }
}

impl From<DesignCombineOperation> for DesignCombineOperationWire {
    fn from(operation: DesignCombineOperation) -> Self {
        Self {
            form: operation.form,
            operation: operation.operation,
            operation_offset: operation.operation_offset,
            keep_tools: operation.keep_tools,
            keep_tools_offset: operation.keep_tools_offset,
            target: DesignCombineBodySelection {
                record_index: operation.target_record_index,
                external_identity: None,
            },
            tools: std::iter::once(operation.tools.first)
                .chain(operation.tools.additional)
                .collect(),
        }
    }
}

/// Thread construction form selected by the scope prefix and payload marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignThreadForm {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact prefix, construction marker, and trailer layout.
    Compact(Option<Located<NonZeroU32>>),
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
}

/// Exact form and size construction carried by a `Thread` scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignThreadConstructionWire")]
pub struct DesignThreadConstruction {
    /// Standard, compact, or class-specific legacy construction form.
    pub form: DesignThreadForm,
    /// Byte offset of the designation LP-UTF16 field.
    pub designation_offset: u64,
    /// Standard thread designation.
    pub designation: cadmpeg_ir::NonEmptyString,
    /// Validated nominal-size spelling; its numeric value is derived on read.
    pub nominal_size: DesignThreadNominalSize,
    /// Thread profile name.
    pub profile: cadmpeg_ir::NonEmptyString,
    /// Ordered physical thread diameters in Design length units.
    pub diameters: DesignThreadDiameters,
    /// Thread pitch in Design length units.
    pub pitch: DesignPositiveScalar,
    /// Ordered counted face-selection groups referenced by the scope.
    pub face_group_record_indices: Vec<u32>,
}

/// Positive finite thread diameters ordered from minor through pitch to major.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignThreadDiameters {
    major: DesignPositiveScalar,
    minor: DesignPositiveScalar,
    pitch: DesignPositiveScalar,
}

impl DesignThreadDiameters {
    /// Admit strictly ordered positive finite thread diameters.
    pub fn new(major: f64, minor: f64, pitch: f64) -> Option<Self> {
        let major = DesignPositiveScalar::new(major)?;
        let minor = DesignPositiveScalar::new(minor)?;
        let pitch = DesignPositiveScalar::new(pitch)?;
        (minor.get() < pitch.get() && pitch.get() < major.get()).then_some(Self {
            major,
            minor,
            pitch,
        })
    }
    /// Physical major diameter in Design length units.
    pub fn major(self) -> f64 {
        self.major.get()
    }
    /// Physical minor diameter in Design length units.
    pub fn minor(self) -> f64 {
        self.minor.get()
    }
    /// Physical pitch diameter in Design length units.
    pub fn pitch(self) -> f64 {
        self.pitch.get()
    }
}

/// Original spelling of a finite positive nominal thread size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignThreadNominalSize(String);

impl TryFrom<String> for DesignThreadNominalSize {
    type Error = String;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if !text
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite() && value > 0.0)
        {
            return Err("nominal_size_text must encode a finite positive number".into());
        }
        Ok(Self(text))
    }
}

impl DesignThreadNominalSize {
    #[must_use]
    #[cfg(test)]
    pub fn text(&self) -> &str {
        &self.0
    }

    pub fn value(&self) -> Result<f64, std::num::ParseFloatError> {
        self.0.parse()
    }
}

impl Serialize for DesignThreadConstruction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignThreadConstructionWire::try_from(self.clone())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesignThreadFormWire {
    /// Standard prefix, construction marker, and trailer layout.
    Standard,
    /// Compact prefix, construction marker, and trailer layout.
    Compact,
    /// Direct standard prefix with the legacy compact scalar and trailer lanes.
    StandardLegacy,
    /// Compact prefix with the legacy scalar and no-reference trailer lanes.
    CompactLegacy,
}

#[derive(Serialize, Deserialize)]
struct DesignThreadConstructionWire {
    /// Standard, compact, or class-specific legacy construction form.
    form: DesignThreadFormWire,
    /// Byte offset of the designation LP-UTF16 field.
    designation_offset: u64,
    /// Standard thread designation.
    designation: String,
    /// Exact nominal-size text interpreted into `nominal_size`.
    nominal_size_text: String,
    /// Numeric nominal size interpreted by `profile`.
    nominal_size: f64,
    /// Thread profile name.
    profile: String,
    /// Physical major diameter in Design length units.
    major_diameter: f64,
    /// Physical minor diameter in Design length units.
    minor_diameter: f64,
    /// Thread pitch in Design length units.
    pitch: f64,
    /// Pitch diameter in Design length units.
    pitch_diameter: f64,
    /// Record named by the reference-bearing compact trailer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_reference_record_index: Option<u32>,
    /// Byte offset of `trailing_reference_record_index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_reference_offset: Option<u64>,
    /// Ordered counted face-selection groups referenced by the scope.
    face_group_record_indices: Vec<u32>,
}

impl TryFrom<DesignThreadConstruction> for DesignThreadConstructionWire {
    type Error = String;
    fn try_from(value: DesignThreadConstruction) -> Result<Self, Self::Error> {
        let nominal_size = value
            .nominal_size
            .value()
            .map_err(|error| format!("nominal_size_text: {error}"))?;
        let (form, trailing_reference) = match value.form {
            DesignThreadForm::Standard => (DesignThreadFormWire::Standard, None),
            DesignThreadForm::Compact(reference) => (DesignThreadFormWire::Compact, reference),
            DesignThreadForm::StandardLegacy => (DesignThreadFormWire::StandardLegacy, None),
            DesignThreadForm::CompactLegacy => (DesignThreadFormWire::CompactLegacy, None),
        };
        Ok(Self {
            form,
            designation_offset: value.designation_offset,
            designation: value.designation.as_str().to_owned(),
            nominal_size_text: value.nominal_size.0,
            nominal_size,
            profile: value.profile.as_str().to_owned(),
            major_diameter: value.diameters.major(),
            minor_diameter: value.diameters.minor(),
            pitch: value.pitch.get(),
            pitch_diameter: value.diameters.pitch(),
            trailing_reference_record_index: trailing_reference.map(|located| located.value.get()),
            trailing_reference_offset: trailing_reference.map(|located| located.offset),
            face_group_record_indices: value.face_group_record_indices,
        })
    }
}

impl TryFrom<DesignThreadConstructionWire> for DesignThreadConstruction {
    type Error = String;
    fn try_from(value: DesignThreadConstructionWire) -> Result<Self, Self::Error> {
        let nominal_size = DesignThreadNominalSize::try_from(value.nominal_size_text)?;
        if nominal_size
            .value()
            .map_err(|error| format!("nominal_size_text: {error}"))?
            .to_bits()
            != value.nominal_size.to_bits()
        {
            return Err("nominal_size must match nominal_size_text".into());
        }
        let reference = match (
            value.trailing_reference_record_index,
            value.trailing_reference_offset,
        ) {
            (None, None) => None,
            (Some(value), Some(offset)) => Some(Located {
                value: NonZeroU32::new(value)
                    .ok_or("trailing_reference_record_index must be nonzero")?,
                offset,
            }),
            _ => return Err(
                "trailing_reference_record_index and trailing_reference_offset must occur together"
                    .into(),
            ),
        };
        let form = match (value.form, reference) {
            (DesignThreadFormWire::Compact, reference) => DesignThreadForm::Compact(reference),
            (DesignThreadFormWire::Standard, None) => DesignThreadForm::Standard,
            (DesignThreadFormWire::StandardLegacy, None) => DesignThreadForm::StandardLegacy,
            (DesignThreadFormWire::CompactLegacy, None) => DesignThreadForm::CompactLegacy,
            _ => {
                return Err("trailing_reference_record_index is only valid for compact form".into())
            }
        };
        Ok(Self {
            form,
            designation_offset: value.designation_offset,
            designation: cadmpeg_ir::NonEmptyString::new(value.designation).ok_or("designation must not be empty")?,
            nominal_size,
            profile: cadmpeg_ir::NonEmptyString::new(value.profile).ok_or("profile must not be empty")?,
            diameters: DesignThreadDiameters::new(value.major_diameter, value.minor_diameter, value.pitch_diameter)
                .ok_or("major_diameter, minor_diameter, and pitch_diameter must be positive finite and strictly ordered")?,
            pitch: DesignPositiveScalar::new(value.pitch).ok_or("pitch must be positive finite")?,
            face_group_record_indices: value.face_group_record_indices,
        })
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

/// Source form for an exact solved `WorkAxis` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DesignWorkAxisSource {
    /// The axis carrier and two endpoint point carriers are cross-checked.
    TwoPoint {
        /// Ordered endpoint carrier record indices.
        point_record_indices: [u32; 2],
        /// Byte offsets of the first coordinate in each endpoint carrier.
        point_offsets: [u64; 2],
    },
    /// A generation-specific carrier stores the axis directly and has one
    /// additional construction-support record in the enclosing scope.
    DirectCarrier {
        /// Indexed record carrying the origin and displacement values.
        carrier_record_index: u32,
        /// Enclosing scope's second ordered construction record.
        support_record_index: u32,
    },
}

/// Exact solved construction carried by a `WorkAxis` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignWorkAxisConstruction {
    /// First construction point in model centimetres.
    pub origin: [f64; 3],
    /// Displacement from the first construction point to the second, in centimetres.
    pub displacement: [f64; 3],
    /// Byte offset of the first origin coordinate.
    pub origin_offset: u64,
    /// Byte offset of the first displacement component.
    pub displacement_offset: u64,
    /// Native record form that supplied or corroborated the axis geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<DesignWorkAxisSource>,
}

/// One source-record reference used by a `WorkPoint` construction rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignWorkPointInputDraft",
    into = "DesignWorkPointInputDraft"
)]
pub struct DesignWorkPointInput {
    /// Referenced Design record index.
    record_index: u32,
    /// Byte offset of the serialized reference target.
    pub reference_offset: u64,
    /// Exact source carrier selected by this reference, when decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carrier: Option<Box<DesignWorkPointInputCarrier>>,
}

impl DesignWorkPointInput {
    pub(crate) fn try_new(draft: DesignWorkPointInputDraft) -> Result<Self, String> {
        match draft.carrier.as_deref() {
            Some(DesignWorkPointInputCarrier::WorkPlane { selection })
                if draft.record_index.checked_add(3) != Some(selection.identity_record_index()) =>
            {
                return Err("carrier.identity_record_index must follow record_index by 3".into())
            }
            Some(DesignWorkPointInputCarrier::SketchPoint { selection })
                if draft.record_index.checked_add(3) != Some(selection.identity_record_index())
                    || draft.record_index.checked_add(4) != Some(selection.next_record_index()) =>
            {
                return Err("carrier identity/next_record_index disagree with input frame".into())
            }
            _ => {}
        }
        let value = Self {
            record_index: draft.record_index,
            reference_offset: draft.reference_offset,
            carrier: draft.carrier,
        };
        Ok(value)
    }
    pub(crate) fn into_draft(self) -> DesignWorkPointInputDraft {
        DesignWorkPointInputDraft {
            record_index: self.record_index,
            reference_offset: self.reference_offset,
            carrier: self.carrier,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn carrier(&self) -> Option<&DesignWorkPointInputCarrier> {
        self.carrier.as_deref()
    }
}

/// Unadmitted `DesignWorkPointInput` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPointInputDraft {
    /// Referenced Design record index.
    pub record_index: u32,
    /// Byte offset of the serialized reference target.
    pub reference_offset: u64,
    /// Exact source carrier selected by this reference, when decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<Box<DesignWorkPointInputCarrier>>,
}

impl TryFrom<DesignWorkPointInputDraft> for DesignWorkPointInput {
    type Error = String;
    fn try_from(draft: DesignWorkPointInputDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}
impl From<DesignWorkPointInput> for DesignWorkPointInputDraft {
    fn from(value: DesignWorkPointInput) -> Self {
        let value = value.into_draft();
        Self {
            record_index: value.record_index,
            reference_offset: value.reference_offset,
            carrier: value.carrier,
        }
    }
}

/// Exact source carrier selected by one `WorkPoint` construction input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignWorkPointInputCarrier {
    /// Persistent edge recipe retained in the native edge-operand arena.
    EdgeRecipe {
        /// Native `DesignEdgeOperand` identifier.
        operand_id: String,
    },
    /// Persistent vertex recipe carried directly by this `WorkPoint` input.
    VertexRecipe {
        /// Exact vertex-recipe envelope.
        recipe: DesignVertexRecipe,
    },
    /// Persistent entity selection naming one `WorkPlane` scope.
    WorkPlane {
        /// Exact selection envelope and resolved `WorkPlane` scope.
        selection: DesignWorkPointPlaneSelection,
    },
    /// Direct persistent selection of one sketch point.
    SketchPoint {
        /// Exact selection envelope and resolved native sketch-point record.
        selection: DesignWorkPointSketchPointSelection,
    },
}

/// Historical state and a nonnegative stable vertex slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignVertexResolution {
    /// Historical topology state against which the vertex recipe was evaluated.
    pub state_id: i64,
    vertex_slot: i64,
}

impl DesignVertexResolution {
    pub fn new(state_id: i64, vertex_slot: i64) -> Option<Self> {
        (vertex_slot >= 0).then_some(Self {
            state_id,
            vertex_slot,
        })
    }

    pub fn vertex_slot(self) -> i64 {
        self.vertex_slot
    }
}

/// Exact persistent `vertex_recipe_data` envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DesignVertexRecipeWire", into = "DesignVertexRecipeWire")]
pub struct DesignVertexRecipe {
    frame: super::frame_chain::RecordFrameChain,
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
    /// Source per-file dynamic paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the vertex-recipe record header.
    recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Complete prefix before the length-prefixed recipe-family name.
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first post-name i32.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program.
    pub recipe_program: Vec<i32>,
    /// Historical state and proven stable vertex slot.
    pub resolution: Option<DesignVertexResolution>,
    /// Byte offset of the indexed record closing the envelope.
    next_byte_offset: u64,
}

impl DesignVertexRecipe {
    pub(crate) fn try_new(draft: DesignVertexRecipeDraft) -> Result<Self, String> {
        if !(draft.byte_offset < draft.paired_byte_offset
            && draft.paired_byte_offset < draft.recipe_record_byte_offset
            && draft.recipe_record_byte_offset < draft.next_byte_offset)
        {
            return Err(
                "paired_byte_offset/recipe_record_byte_offset/next_byte_offset must increase"
                    .into(),
            );
        }
        draft
            .recipe_record_byte_offset
            .checked_add(11)
            .ok_or("recipe_prefix_offset overflows")?;
        let frame = super::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            5,
            0,
        )?;
        let value = Self {
            frame,
            class_tag: draft.class_tag,
            paired_byte_offset: draft.paired_byte_offset,
            paired_class_tag: draft.paired_class_tag,
            recipe_record_byte_offset: draft.recipe_record_byte_offset,
            recipe_id: draft.recipe_id,
            recipe_prefix_bytes: draft.recipe_prefix_bytes,
            recipe_references: draft.recipe_references,
            recipe_program_offset: draft.recipe_program_offset,
            recipe_program: draft.recipe_program,
            resolution: draft.resolution,
            next_byte_offset: draft.next_byte_offset,
        };
        if value.recipe_record_index() != draft.recipe_record_index {
            return Err("recipe_record_index disagrees with frame layout".into());
        }
        if value.recipe_prefix_offset() != draft.recipe_prefix_offset {
            return Err("recipe_prefix_offset disagrees with frame layout".into());
        }
        if value.next_record_index() != draft.next_record_index {
            return Err("next_record_index disagrees with frame layout".into());
        }
        Ok(value)
    }
    pub(crate) fn into_draft(self) -> DesignVertexRecipeDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let recipe_record_index = self.recipe_record_index();
        let recipe_prefix_offset = self.recipe_prefix_offset();
        let next_record_index = self.next_record_index();
        DesignVertexRecipeDraft {
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            paired_byte_offset: self.paired_byte_offset,
            paired_class_tag: self.paired_class_tag,
            recipe_record_index,
            recipe_record_byte_offset: self.recipe_record_byte_offset,
            recipe_id: self.recipe_id,
            recipe_prefix_offset,
            recipe_prefix_bytes: self.recipe_prefix_bytes,
            recipe_references: self.recipe_references,
            recipe_program_offset: self.recipe_program_offset,
            recipe_program: self.recipe_program,
            resolution: self.resolution,
            next_record_index,
            next_byte_offset: self.next_byte_offset,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn recipe_record_index(&self) -> u32 {
        self.frame.index(3)
    }
    pub(crate) fn recipe_record_byte_offset(&self) -> u64 {
        self.recipe_record_byte_offset
    }
    pub(crate) fn recipe_prefix_offset(&self) -> u64 {
        self.recipe_record_byte_offset + 11
    }
    pub(crate) fn next_record_index(&self) -> u32 {
        self.frame.index(5)
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.next_byte_offset
    }
}

/// Unadmitted `DesignVertexRecipe` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignVertexRecipeDraft {
    /// Indexed record that owns the vertex-recipe envelope.
    pub record_index: u32,
    /// Byte offset of the owning indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Indexed record containing the vertex recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the vertex-recipe record header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete prefix before the length-prefixed recipe-family name.
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first post-name i32.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program.
    pub recipe_program: Vec<i32>,
    /// Historical state and proven stable vertex slot.
    pub resolution: Option<DesignVertexResolution>,
    /// Identity of the indexed record closing the envelope.
    pub next_record_index: u32,
    /// Byte offset of the indexed record closing the envelope.
    pub next_byte_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignVertexRecipeWire {
    /// Indexed record that owns the vertex-recipe envelope.
    record_index: u32,
    /// Byte offset of the owning indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
    /// Source per-file dynamic paired class tag.
    paired_class_tag: String,
    /// Indexed record containing the vertex recipe.
    recipe_record_index: u32,
    /// Byte offset of the vertex-recipe record header.
    recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    recipe_prefix_offset: u64,
    /// Complete prefix before the length-prefixed recipe-family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    recipe_prefix_bytes: Vec<u8>,
    /// Persistent selector/reference entries decoded from the prefix.
    recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first post-name i32.
    recipe_program_offset: u64,
    /// Complete post-name i32 program.
    recipe_program: Vec<i32>,
    /// Historical topology state against which the vertex recipe was evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe_state_id: Option<i64>,
    /// Stable vertex slot proven by the persistent face references and solved point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_vertex_slot: Option<i64>,
    /// Identity of the indexed record closing the envelope.
    next_record_index: u32,
    /// Byte offset of the indexed record closing the envelope.
    next_byte_offset: u64,
}

impl From<DesignVertexRecipe> for DesignVertexRecipeWire {
    fn from(value: DesignVertexRecipe) -> Self {
        let value = value.into_draft();
        Self {
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag.into(),
            paired_byte_offset: value.paired_byte_offset,
            paired_class_tag: value.paired_class_tag.into(),
            recipe_record_index: value.recipe_record_index,
            recipe_record_byte_offset: value.recipe_record_byte_offset,
            recipe_id: value.recipe_id,
            recipe_prefix_offset: value.recipe_prefix_offset,
            recipe_prefix_bytes: value.recipe_prefix_bytes,
            recipe_references: value.recipe_references,
            recipe_program_offset: value.recipe_program_offset,
            recipe_program: value.recipe_program,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
            recipe_state_id: value.resolution.map(|resolution| resolution.state_id),
            resolved_vertex_slot: value.resolution.map(DesignVertexResolution::vertex_slot),
        }
    }
}

impl TryFrom<DesignVertexRecipeWire> for DesignVertexRecipe {
    type Error = String;

    fn try_from(value: DesignVertexRecipeWire) -> Result<Self, Self::Error> {
        let resolution = match (value.recipe_state_id, value.resolved_vertex_slot) {
            (None, None) => None,
            (Some(state_id), Some(vertex_slot)) => Some(
                DesignVertexResolution::new(state_id, vertex_slot)
                    .ok_or("resolved_vertex_slot must be nonnegative")?,
            ),
            _ => {
                return Err(
                    "recipe_state_id and resolved_vertex_slot must be present together".into(),
                )
            }
        };
        Self::try_new(DesignVertexRecipeDraft {
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag.try_into()?,
            paired_byte_offset: value.paired_byte_offset,
            paired_class_tag: value.paired_class_tag.try_into()?,
            recipe_record_index: value.recipe_record_index,
            recipe_record_byte_offset: value.recipe_record_byte_offset,
            recipe_id: value.recipe_id,
            recipe_prefix_offset: value.recipe_prefix_offset,
            recipe_prefix_bytes: value.recipe_prefix_bytes,
            recipe_references: value.recipe_references,
            recipe_program_offset: value.recipe_program_offset,
            recipe_program: value.recipe_program,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
            resolution,
        })
    }
}

/// Corner-vertex recipe carried as one member of an edge-treatment group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignEdgeTreatmentVertexOperand {
    /// Globally unique deterministic identifier for this group member.
    pub id: String,
    /// Owning edge-treatment scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope reference table.
    pub scope_reference_ordinal: u32,
    /// Owning counted construction group.
    pub group_record_index: u32,
    /// Zero-based position in the group's member run.
    pub group_member_ordinal: u32,
    /// Exact persistent vertex-recipe envelope and resolved historical corner.
    pub recipe: DesignVertexRecipe,
}

/// Plane through three persistent B-rep vertices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignWorkPlaneConstructionWire",
    into = "DesignWorkPlaneConstructionWire"
)]
pub struct DesignWorkPlaneConstruction {
    /// Solved placement-frame record named by the scope.
    pub placement_record_index: u32,
    /// Persistent vertex inputs in source order.
    inputs: Box<[DesignVertexRecipe; 3]>,
}

impl DesignWorkPlaneConstruction {
    /// Admit an unresolved triple or three distinct vertices from one history state.
    pub fn try_new(
        placement_record_index: u32,
        inputs: Box<[DesignVertexRecipe; 3]>,
    ) -> Result<Self, String> {
        validate_three_point_resolutions(inputs.each_ref().map(|input| input.resolution))?;
        Ok(Self {
            placement_record_index,
            inputs,
        })
    }

    /// Persistent vertex recipes in source order.
    pub fn inputs(&self) -> &[DesignVertexRecipe; 3] {
        &self.inputs
    }

    /// Candidate references without mutable access to vertex resolutions.
    pub(crate) fn recipe_references_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut DesignRecipeReference> {
        self.inputs
            .iter_mut()
            .flat_map(|recipe| recipe.recipe_references.iter_mut())
    }

    /// Remove resolution for the complete vertex triple.
    pub(crate) fn clear_resolution(&mut self) {
        for input in self.inputs.iter_mut() {
            input.resolution = None;
        }
    }

    /// Commit three distinct vertices from one state, preserving the old value on failure.
    pub(crate) fn try_set_resolution(
        &mut self,
        resolution: [DesignVertexResolution; 3],
    ) -> Result<(), String> {
        validate_three_point_resolutions(resolution.map(Some))?;
        for (input, resolution) in self.inputs.iter_mut().zip(resolution) {
            input.resolution = Some(resolution);
        }
        Ok(())
    }
}

fn validate_three_point_resolutions(
    resolutions: [Option<DesignVertexResolution>; 3],
) -> Result<(), String> {
    match resolutions {
        [None, None, None] => Ok(()),
        [Some(first), Some(second), Some(third)] => {
            if first.state_id != second.state_id || first.state_id != third.state_id {
                return Err("recipe_state_id must match across all three inputs".into());
            }
            if first.vertex_slot() == second.vertex_slot()
                || first.vertex_slot() == third.vertex_slot()
                || second.vertex_slot() == third.vertex_slot()
            {
                return Err("resolved_vertex_slot must be distinct across all three inputs".into());
            }
            Ok(())
        }
        _ => Err(
            "inputs resolution must be absent for all three vertices or present for all three"
                .into(),
        ),
    }
}

/// Wire form of [`DesignWorkPlaneConstruction`].
///
/// The single variant is deliberate. It stamps `"kind": "three_point"` into
/// the native JSON, keeping room for the other `WorkPlane` constructions
/// Fusion authors, and — unlike an internally tagged struct, which ignores
/// the tag field — it rejects any other `kind` on deserialization.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignWorkPlaneConstructionWire {
    ThreePoint {
        placement_record_index: u32,
        inputs: Box<[DesignVertexRecipe; 3]>,
    },
}

impl From<DesignWorkPlaneConstruction> for DesignWorkPlaneConstructionWire {
    fn from(value: DesignWorkPlaneConstruction) -> Self {
        Self::ThreePoint {
            placement_record_index: value.placement_record_index,
            inputs: value.inputs,
        }
    }
}

impl TryFrom<DesignWorkPlaneConstructionWire> for DesignWorkPlaneConstruction {
    type Error = String;
    fn try_from(value: DesignWorkPlaneConstructionWire) -> Result<Self, Self::Error> {
        let DesignWorkPlaneConstructionWire::ThreePoint {
            placement_record_index,
            inputs,
        } = value;
        Self::try_new(placement_record_index, inputs)
    }
}

/// Exact persistent entity selection naming one `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignWorkPointPlaneSelectionDraft",
    into = "DesignWorkPointPlaneSelectionDraft"
)]
pub struct DesignWorkPointPlaneSelection {
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Serialized primary identity immediately preceding the `WorkPlane` scope.
    pub primary_identity: u64,
    /// Selected `WorkPlane` scope record index.
    pub work_plane_scope_record_index: u32,
    /// Identity of the indexed record closing the selection envelope.
    next_record_index: u32,
}

impl DesignWorkPointPlaneSelection {
    pub(crate) fn try_new(draft: DesignWorkPointPlaneSelectionDraft) -> Result<Self, String> {
        draft
            .identity_record_offset
            .checked_add(29)
            .ok_or("identity_record_offset overflows")?;
        if !(draft.asset_id_offset < draft.context_id_offset
            && draft.context_id_offset < draft.identity_record_offset)
        {
            return Err(
                "asset_id_offset/context_id_offset/identity_record_offset must increase".into(),
            );
        }
        let value = Self {
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            asset_id_offset: draft.asset_id_offset,
            context_id: draft.context_id,
            context_id_offset: draft.context_id_offset,
            identity_record_index: draft.identity_record_index,
            identity_record_offset: draft.identity_record_offset,
            primary_identity: draft.primary_identity,
            work_plane_scope_record_index: draft.work_plane_scope_record_index,
            next_record_index: draft.next_record_index,
        };
        if value.next_byte_offset() != draft.next_byte_offset {
            return Err("next_byte_offset disagrees with frame layout".into());
        }
        if value.primary_identity_offset() != draft.primary_identity_offset {
            return Err("primary_identity_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    pub(crate) fn into_draft(self) -> DesignWorkPointPlaneSelectionDraft {
        let next_byte_offset = self.next_byte_offset();
        let primary_identity_offset = self.primary_identity_offset();
        DesignWorkPointPlaneSelectionDraft {
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset: self.asset_id_offset,
            context_id: self.context_id,
            context_id_offset: self.context_id_offset,
            identity_record_index: self.identity_record_index,
            identity_record_offset: self.identity_record_offset,
            primary_identity: self.primary_identity,
            primary_identity_offset,
            work_plane_scope_record_index: self.work_plane_scope_record_index,
            next_record_index: self.next_record_index,
            next_byte_offset,
        }
    }
    pub(crate) fn asset_id_offset(&self) -> u64 {
        self.asset_id_offset
    }
    pub(crate) fn identity_record_index(&self) -> u32 {
        self.identity_record_index
    }
    pub(crate) fn primary_identity_offset(&self) -> u64 {
        self.identity_record_offset + 21
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.identity_record_offset + 29
    }
}

/// Unadmitted `DesignWorkPointPlaneSelection` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPointPlaneSelectionDraft {
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Serialized primary identity immediately preceding the `WorkPlane` scope.
    pub primary_identity: u64,
    /// Byte offset of the primary identity.
    pub primary_identity_offset: u64,
    /// Selected `WorkPlane` scope record index.
    pub work_plane_scope_record_index: u32,
    /// Identity of the indexed record closing the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the indexed record closing the selection envelope.
    pub next_byte_offset: u64,
}

impl TryFrom<DesignWorkPointPlaneSelectionDraft> for DesignWorkPointPlaneSelection {
    type Error = String;
    fn try_from(draft: DesignWorkPointPlaneSelectionDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}
impl From<DesignWorkPointPlaneSelection> for DesignWorkPointPlaneSelectionDraft {
    fn from(value: DesignWorkPointPlaneSelection) -> Self {
        let value = value.into_draft();
        Self {
            class_tag: value.class_tag,
            asset_id: value.asset_id,
            asset_id_offset: value.asset_id_offset,
            context_id: value.context_id,
            context_id_offset: value.context_id_offset,
            identity_record_index: value.identity_record_index,
            identity_record_offset: value.identity_record_offset,
            primary_identity: value.primary_identity,
            primary_identity_offset: value.primary_identity_offset,
            work_plane_scope_record_index: value.work_plane_scope_record_index,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

/// Exact persistent entity selection naming one sketch point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignWorkPointSketchPointSelectionDraft",
    into = "DesignWorkPointSketchPointSelectionDraft"
)]
pub struct DesignWorkPointSketchPointSelection {
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Record identity of the owning Sketch entity.
    pub sketch_record_index: u32,
    /// Persistent identity of the selected sketch point.
    pub point_persistent_id: u64,
    /// Native id of the decoded sketch-point record selected by this frame.
    pub point_native_id: String,
    /// Identity of the indexed record closing the selection envelope.
    next_record_index: u32,
}

impl DesignWorkPointSketchPointSelection {
    pub(crate) fn try_new(draft: DesignWorkPointSketchPointSelectionDraft) -> Result<Self, String> {
        draft
            .identity_record_offset
            .checked_add(crate::layout::work_point_sketch_point_identity::LEN as u64)
            .ok_or("identity_record_offset overflows")?;
        if !(draft.asset_id_offset < draft.context_id_offset
            && draft.context_id_offset < draft.identity_record_offset)
        {
            return Err(
                "asset_id_offset/context_id_offset/identity_record_offset must increase".into(),
            );
        }
        let value = Self {
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            asset_id_offset: draft.asset_id_offset,
            context_id: draft.context_id,
            context_id_offset: draft.context_id_offset,
            identity_record_index: draft.identity_record_index,
            identity_record_offset: draft.identity_record_offset,
            sketch_record_index: draft.sketch_record_index,
            point_persistent_id: draft.point_persistent_id,
            point_native_id: draft.point_native_id,
            next_record_index: draft.next_record_index,
        };
        if value.next_byte_offset() != draft.next_byte_offset {
            return Err("next_byte_offset disagrees with frame layout".into());
        }
        if value.sketch_record_index_offset() != draft.sketch_record_index_offset {
            return Err("sketch_record_index_offset disagrees with frame layout".into());
        }
        if value.point_persistent_id_offset() != draft.point_persistent_id_offset {
            return Err("point_persistent_id_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    pub(crate) fn into_draft(self) -> DesignWorkPointSketchPointSelectionDraft {
        let next_byte_offset = self.next_byte_offset();
        let sketch_record_index_offset = self.sketch_record_index_offset();
        let point_persistent_id_offset = self.point_persistent_id_offset();
        DesignWorkPointSketchPointSelectionDraft {
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset: self.asset_id_offset,
            context_id: self.context_id,
            context_id_offset: self.context_id_offset,
            identity_record_index: self.identity_record_index,
            identity_record_offset: self.identity_record_offset,
            sketch_record_index: self.sketch_record_index,
            sketch_record_index_offset,
            point_persistent_id: self.point_persistent_id,
            point_persistent_id_offset,
            point_native_id: self.point_native_id,
            next_record_index: self.next_record_index,
            next_byte_offset,
        }
    }
    pub(crate) fn asset_id_offset(&self) -> u64 {
        self.asset_id_offset
    }
    pub(crate) fn identity_record_index(&self) -> u32 {
        self.identity_record_index
    }
    pub(crate) fn sketch_record_index_offset(&self) -> u64 {
        self.identity_record_offset
            + crate::layout::work_point_sketch_point_identity::SKETCH_RECORD_INDEX as u64
    }
    pub(crate) fn point_persistent_id_offset(&self) -> u64 {
        self.identity_record_offset
            + crate::layout::work_point_sketch_point_identity::POINT_PERSISTENT_ID as u64
    }
    pub(crate) fn next_record_index(&self) -> u32 {
        self.next_record_index
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.identity_record_offset + crate::layout::work_point_sketch_point_identity::LEN as u64
    }
}

/// Unadmitted `DesignWorkPointSketchPointSelection` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPointSketchPointSelectionDraft {
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Record identity of the owning Sketch entity.
    pub sketch_record_index: u32,
    /// Byte offset of the Sketch entity identity.
    pub sketch_record_index_offset: u64,
    /// Persistent identity of the selected sketch point.
    pub point_persistent_id: u64,
    /// Byte offset of the sketch-point identity.
    pub point_persistent_id_offset: u64,
    /// Native id of the decoded sketch-point record selected by this frame.
    pub point_native_id: String,
    /// Identity of the indexed record closing the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the indexed record closing the selection envelope.
    pub next_byte_offset: u64,
}

impl TryFrom<DesignWorkPointSketchPointSelectionDraft> for DesignWorkPointSketchPointSelection {
    type Error = String;
    fn try_from(draft: DesignWorkPointSketchPointSelectionDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}
impl From<DesignWorkPointSketchPointSelection> for DesignWorkPointSketchPointSelectionDraft {
    fn from(value: DesignWorkPointSketchPointSelection) -> Self {
        let value = value.into_draft();
        Self {
            class_tag: value.class_tag,
            asset_id: value.asset_id,
            asset_id_offset: value.asset_id_offset,
            context_id: value.context_id,
            context_id_offset: value.context_id_offset,
            identity_record_index: value.identity_record_index,
            identity_record_offset: value.identity_record_offset,
            sketch_record_index: value.sketch_record_index,
            sketch_record_index_offset: value.sketch_record_index_offset,
            point_persistent_id: value.point_persistent_id,
            point_persistent_id_offset: value.point_persistent_id_offset,
            point_native_id: value.point_native_id,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

/// Construction rule whose input arity and decoded carrier roles agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DesignWorkPointRuleForm", into = "DesignWorkPointRuleForm")]
pub struct DesignWorkPointRule {
    form: DesignWorkPointRuleForm,
}

impl DesignWorkPointRule {
    pub(crate) fn from_serialized(
        reference_type: u32,
        inputs: Vec<DesignWorkPointInput>,
    ) -> Result<Self, String> {
        let form = match (reference_type, inputs.as_slice()) {
            (5, [input]) => DesignWorkPointRuleForm::CircleCenter {
                input: input.clone(),
            },
            (7, [first, second]) => DesignWorkPointRuleForm::TwoEdgeIntersection {
                inputs: [first.clone(), second.clone()],
            },
            (8, [first, second, third]) => DesignWorkPointRuleForm::ThreePlaneIntersection {
                inputs: [first.clone(), second.clone(), third.clone()],
            },
            (10, [input]) => DesignWorkPointRuleForm::Vertex {
                input: input.clone(),
            },
            (14, [first, second]) => DesignWorkPointRuleForm::EdgePlaneIntersection {
                inputs: [first.clone(), second.clone()],
            },
            (20, [input]) => DesignWorkPointRuleForm::DistanceOnEdge {
                input: input.clone(),
            },
            _ => DesignWorkPointRuleForm::Native {
                reference_type,
                inputs,
            },
        };
        let is_edge = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(carrier, DesignWorkPointInputCarrier::EdgeRecipe { .. })
            })
        };
        let is_vertex = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(
                    carrier,
                    DesignWorkPointInputCarrier::VertexRecipe { .. }
                        | DesignWorkPointInputCarrier::SketchPoint { .. }
                )
            })
        };
        let is_plane = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(carrier, DesignWorkPointInputCarrier::WorkPlane { .. })
            })
        };
        let compatible = match &form {
            DesignWorkPointRuleForm::CircleCenter { input }
            | DesignWorkPointRuleForm::DistanceOnEdge { input } => is_edge(input),
            DesignWorkPointRuleForm::TwoEdgeIntersection { inputs } => inputs.iter().all(is_edge),
            DesignWorkPointRuleForm::ThreePlaneIntersection { inputs } => {
                inputs.iter().all(is_plane)
            }
            DesignWorkPointRuleForm::Vertex { input } => is_vertex(input),
            DesignWorkPointRuleForm::EdgePlaneIntersection { inputs } => {
                is_edge(&inputs[0]) && is_plane(&inputs[1])
            }
            DesignWorkPointRuleForm::Native { .. } => true,
        };
        if !compatible {
            return Err(
                "WorkPoint rule input carrier conflicts with its reference_type role".into(),
            );
        }
        Ok(Self { form })
    }

    pub fn form(&self) -> &DesignWorkPointRuleForm {
        &self.form
    }

    pub fn reference_type(&self) -> u32 {
        self.form.reference_type()
    }

    pub fn inputs(&self) -> &[DesignWorkPointInput] {
        self.form.inputs()
    }

    pub(crate) fn vertex_recipes_mut(&mut self) -> impl Iterator<Item = &mut DesignVertexRecipe> {
        self.form.inputs_mut().iter_mut().filter_map(|input| {
            match input.carrier.as_deref_mut()? {
                DesignWorkPointInputCarrier::VertexRecipe { recipe } => Some(recipe),
                _ => None,
            }
        })
    }
}

impl From<DesignWorkPointRule> for DesignWorkPointRuleForm {
    fn from(value: DesignWorkPointRule) -> Self {
        value.form
    }
}

impl TryFrom<DesignWorkPointRuleForm> for DesignWorkPointRule {
    type Error = String;

    fn try_from(value: DesignWorkPointRuleForm) -> Result<Self, Self::Error> {
        let canonical = Self::from_serialized(value.reference_type(), value.inputs().to_vec())?;
        if std::mem::discriminant(&value) != std::mem::discriminant(canonical.form()) {
            return Err(
                "WorkPoint native rule reference_type and input arity identify a supported rule"
                    .into(),
            );
        }
        Ok(canonical)
    }
}

/// Construction rule and exact input arity carried by a `WorkPoint` point-data record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignWorkPointRuleForm {
    /// Center of one selected circular edge.
    CircleCenter {
        /// Selected circular-edge carrier.
        input: DesignWorkPointInput,
    },
    /// Intersection of two selected edges.
    TwoEdgeIntersection {
        /// Selected edge carriers in source order.
        inputs: [DesignWorkPointInput; 2],
    },
    /// Intersection of three selected planes.
    ThreePlaneIntersection {
        /// Selected plane carriers in source order.
        inputs: [DesignWorkPointInput; 3],
    },
    /// One selected B-rep vertex.
    Vertex {
        /// Selected vertex carrier.
        input: DesignWorkPointInput,
    },
    /// Intersection of one selected edge and one selected plane, in source order.
    EdgePlaneIntersection {
        /// Edge and plane carriers in serialized order.
        inputs: [DesignWorkPointInput; 2],
    },
    /// Point at a specified distance along one selected edge.
    DistanceOnEdge {
        /// Selected edge carrier.
        input: DesignWorkPointInput,
    },
    /// Rule code whose operation semantics or input arity is not assigned.
    Native {
        /// Serialized `refType` value.
        reference_type: u32,
        /// Counted input-reference run in source order.
        inputs: Vec<DesignWorkPointInput>,
    },
}

impl DesignWorkPointRuleForm {
    /// Return the serialized `refType` value.
    pub fn reference_type(&self) -> u32 {
        match self {
            Self::CircleCenter { .. } => 5,
            Self::TwoEdgeIntersection { .. } => 7,
            Self::ThreePlaneIntersection { .. } => 8,
            Self::Vertex { .. } => 10,
            Self::EdgePlaneIntersection { .. } => 14,
            Self::DistanceOnEdge { .. } => 20,
            Self::Native { reference_type, .. } => *reference_type,
        }
    }

    /// Return the source input references in serialized order.
    pub fn inputs(&self) -> &[DesignWorkPointInput] {
        match self {
            Self::CircleCenter { input }
            | Self::Vertex { input }
            | Self::DistanceOnEdge { input } => std::slice::from_ref(input),
            Self::TwoEdgeIntersection { inputs } | Self::EdgePlaneIntersection { inputs } => inputs,
            Self::ThreePlaneIntersection { inputs } => inputs,
            Self::Native { inputs, .. } => inputs,
        }
    }

    fn inputs_mut(&mut self) -> &mut [DesignWorkPointInput] {
        match self {
            Self::CircleCenter { input }
            | Self::Vertex { input }
            | Self::DistanceOnEdge { input } => std::slice::from_mut(input),
            Self::TwoEdgeIntersection { inputs } | Self::EdgePlaneIntersection { inputs } => inputs,
            Self::ThreePlaneIntersection { inputs } => inputs,
            Self::Native { inputs, .. } => inputs,
        }
    }
}

/// Exact solved construction carried by a `WorkPoint` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignWorkPointConstruction {
    /// Point-data record selected by the scope.
    pub point_record_index: u32,
    /// Byte offset of the point-data record header.
    pub point_record_byte_offset: u64,
    /// Solved point in source model centimetres.
    pub position: [f64; 3],
    /// Byte offset of the first position coordinate.
    pub position_offset: u64,
    /// Typed construction rule and its source inputs.
    pub rule: DesignWorkPointRule,
    /// Byte offset of the serialized `refType` value.
    pub reference_type_offset: u64,
}

/// Tangent-point payload of a version-four Hole point carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignHoleTangentPoint {
    pub prefix: u8,
    pub data: Located<[f64; 3]>,
}

/// Exact point-and-direction construction carried by a `Hole` scope.
///
/// The native point carrier stores the coordinates in source centimetres and
/// the direction as a unit model-space vector. The remaining fields preserve
/// the carrier's base-level evidence so later Hole forms can bind their input
/// records without reparsing the byte stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignHoleConstructionWire",
    into = "DesignHoleConstructionWire"
)]
pub struct DesignHoleConstruction {
    /// Point-data record selected by the Hole scope.
    pub point_record_index: u32,
    /// Byte offset of the point-data record header.
    pub point_record_byte_offset: u64,
    /// Hole entry position in source model centimetres.
    pub position: [f64; 3],
    /// Byte offset of the first position coordinate.
    pub position_offset: u64,
    /// Directed drilling vector in model space.
    pub direction: [f64; 3],
    /// Byte offset of the first direction component.
    pub direction_offset: u64,
    /// Two point-construction parameters carried by the point-data base level.
    pub point_parameters: [f64; 2],
    /// Byte offsets of the two point-construction parameters.
    pub point_parameter_offsets: [u64; 2],
    /// `refType` construction rule carried by the point-data record.
    pub reference_type: u32,
    /// Byte offset of `reference_type`.
    pub reference_type_offset: u64,
    /// Version-four tangent-point data with its prefix and source location.
    pub tangent_point_data: Option<DesignHoleTangentPoint>,
    /// Located targets of the counted input-reference run.
    pub input_records: Vec<Located<u32>>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_selection: Option<DesignHoleFaceSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHoleConstructionWire {
    /// Point-data record selected by the Hole scope.
    point_record_index: u32,
    /// Byte offset of the point-data record header.
    point_record_byte_offset: u64,
    /// Hole entry position in source model centimetres.
    position: [f64; 3],
    /// Byte offset of the first position coordinate.
    position_offset: u64,
    /// Directed drilling vector in model space.
    direction: [f64; 3],
    /// Byte offset of the first direction component.
    direction_offset: u64,
    /// Two point-construction parameters carried by the point-data base level.
    point_parameters: [f64; 2],
    /// Byte offsets of the two point-construction parameters.
    point_parameter_offsets: [u64; 2],
    /// `refType` construction rule carried by the point-data record.
    reference_type: u32,
    /// Byte offset of `reference_type`.
    reference_type_offset: u64,
    /// Tangent-point data carried by the version-four point-data base level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangent_point_data: Option<[f64; 3]>,
    /// Serialized byte immediately before the version-four tangent-point data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangent_point_data_prefix: Option<u8>,
    /// Byte offset of the first version-four tangent-point component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangent_point_data_offset: Option<u64>,
    /// Record indices of the counted input-reference run.
    input_record_indices: Vec<u32>,
    /// Byte offsets of the input-reference targets.
    input_record_offsets: Vec<u64>,
    /// Direct persistent face selection carried by the Hole scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_selection: Option<DesignHoleFaceSelection>,
}

impl TryFrom<DesignHoleConstructionWire> for DesignHoleConstruction {
    type Error = String;
    fn try_from(wire: DesignHoleConstructionWire) -> Result<Self, Self::Error> {
        if wire.input_record_indices.len() != wire.input_record_offsets.len() {
            return Err(
                "input_record_indices and input_record_offsets must have equal lengths".into(),
            );
        }
        Ok(Self {
            point_record_index: wire.point_record_index,
            point_record_byte_offset: wire.point_record_byte_offset,
            position: wire.position,
            position_offset: wire.position_offset,
            direction: wire.direction,
            direction_offset: wire.direction_offset,
            point_parameters: wire.point_parameters,
            point_parameter_offsets: wire.point_parameter_offsets,
            reference_type: wire.reference_type,
            reference_type_offset: wire.reference_type_offset,
            tangent_point_data: match (wire.tangent_point_data, wire.tangent_point_data_prefix, wire.tangent_point_data_offset) {
                (None, None, None) => None,
                (Some(value), Some(prefix), Some(offset)) => Some(DesignHoleTangentPoint { prefix, data: Located { value, offset } }),
                _ => return Err("tangent_point_data, tangent_point_data_prefix and tangent_point_data_offset must occur together".into()),
            },
            input_records: wire.input_record_indices.into_iter().zip(wire.input_record_offsets).map(|(value, offset)| Located { value, offset }).collect(),
            face_selection: wire.face_selection,
        })
    }
}

impl From<DesignHoleConstruction> for DesignHoleConstructionWire {
    fn from(record: DesignHoleConstruction) -> Self {
        Self {
            point_record_index: record.point_record_index,
            point_record_byte_offset: record.point_record_byte_offset,
            position: record.position,
            position_offset: record.position_offset,
            direction: record.direction,
            direction_offset: record.direction_offset,
            point_parameters: record.point_parameters,
            point_parameter_offsets: record.point_parameter_offsets,
            reference_type: record.reference_type,
            reference_type_offset: record.reference_type_offset,
            tangent_point_data: record
                .tangent_point_data
                .as_ref()
                .map(|tangent| tangent.data.value),
            tangent_point_data_prefix: record
                .tangent_point_data
                .as_ref()
                .map(|tangent| tangent.prefix),
            tangent_point_data_offset: record
                .tangent_point_data
                .as_ref()
                .map(|tangent| tangent.data.offset),
            input_record_indices: record
                .input_records
                .iter()
                .map(|reference| reference.value)
                .collect(),
            input_record_offsets: record
                .input_records
                .iter()
                .map(|reference| reference.offset)
                .collect(),
            face_selection: record.face_selection,
        }
    }
}

/// Direct persistent face selection carried by a `Hole` scope.
///
/// Hole selections are scope references rather than construction-group
/// members. Their envelope is the same persistent entity-selection grammar
/// used by grouped operands, but the scope owns the selection directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignHoleFaceSelectionWire",
    into = "DesignHoleFaceSelectionWire"
)]
pub struct DesignHoleFaceSelection {
    /// Indexed record carrying the persistent selection envelope.
    pub record_index: u32,
    /// Byte offset of the selection envelope header.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Primary persistent identity of the selected face.
    pub primary_identity: u64,
    /// Byte offset of the primary persistent identity.
    pub primary_identity_offset: u64,
    /// Secondary identity and any dependent curve identity, with their source locations.
    pub secondary: Option<DesignSecondaryIdentity<Located<u64>>>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    pub next_record_index: u32,
    /// Byte offset of the following indexed record.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHoleFaceSelectionWire {
    /// Indexed record carrying the persistent selection envelope.
    record_index: u32,
    /// Byte offset of the selection envelope header.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary persistent identity of the selected face.
    primary_identity: u64,
    /// Byte offset of the primary persistent identity.
    primary_identity_offset: u64,
    /// Optional secondary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity: Option<u64>,
    /// Byte offset of the optional secondary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity_offset: Option<u64>,
    /// Optional secondary identity of a selected Sketch curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity: Option<u64>,
    /// Byte offset of the optional Sketch-curve secondary identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity_offset: Option<u64>,
    /// History-qualified face proofs for the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Indexed record immediately following the selection envelope.
    next_record_index: u32,
    /// Byte offset of the following indexed record.
    next_byte_offset: u64,
}

impl TryFrom<DesignHoleFaceSelectionWire> for DesignHoleFaceSelection {
    type Error = String;
    fn try_from(wire: DesignHoleFaceSelectionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id,
            context_id_offset: wire.context_id_offset,
            identity_record_index: wire.identity_record_index,
            identity_record_offset: wire.identity_record_offset,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            secondary: DesignSecondaryIdentity::from_wire(
                Located::from_wire(
                    wire.secondary_identity,
                    wire.secondary_identity_offset,
                    "secondary_identity",
                )?,
                Located::from_wire(
                    wire.curve_secondary_identity,
                    wire.curve_secondary_identity_offset,
                    "curve_secondary_identity",
                )?,
            )?,
            historical_face_candidates: wire.historical_face_candidates,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignHoleFaceSelection> for DesignHoleFaceSelectionWire {
    fn from(record: DesignHoleFaceSelection) -> Self {
        Self {
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            asset_id: record.asset_id,
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id,
            context_id_offset: record.context_id_offset,
            identity_record_index: record.identity_record_index,
            identity_record_offset: record.identity_record_offset,
            primary_identity: record.primary_identity,
            primary_identity_offset: record.primary_identity_offset,
            secondary_identity: record.secondary.map(|secondary| secondary.identity.value),
            secondary_identity_offset: record.secondary.map(|secondary| secondary.identity.offset),
            curve_secondary_identity: record
                .secondary
                .and_then(|secondary| secondary.curve_identity)
                .map(|identity| identity.value),
            curve_secondary_identity_offset: record
                .secondary
                .and_then(|secondary| secondary.curve_identity)
                .map(|identity| identity.offset),
            historical_face_candidates: record.historical_face_candidates,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Nonempty source spelling outside the specialized feature-family names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DesignNativeFeatureName(std::sync::Arc<str>);

macro_rules! design_feature_kinds {
    (data { $($variant:ident => $lit:literal : $payload:ty),+ $(,)? }
     fixed { $($fixed:ident => $fixed_lit:literal : $fixed_payload:ty),+ $(,)? }
     required { $($required:ident => $required_lit:literal : $required_payload:ty),+ $(,)? }
     names { $($unit:ident => $unit_lit:literal),+ $(,)? }) => {
        /// Source feature-family name stored on a parameter scope.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub enum DesignFeatureKind {
            $($variant,)+
            $($fixed,)+
            $($required,)+
            $($unit,)+
            /// Source name without a specialized construction grammar.
            Native(DesignNativeFeatureName),
        }

        impl DesignFeatureKind {
            /// Source spelling written on the wire.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $lit,)+
                    $(Self::$fixed => $fixed_lit,)+
                    $(Self::$required => $required_lit,)+
                    $(Self::$unit => $unit_lit,)+
                    Self::Native(name) => &name.0,
                }
            }

        }

        impl TryFrom<String> for DesignFeatureKind {
            type Error = &'static str;

            fn try_from(name: String) -> Result<Self, Self::Error> {
                match name.as_str() {
                    "" => Err("Design feature kind must not be empty"),
                    $($lit => Ok(Self::$variant),)+
                    $($fixed_lit => Ok(Self::$fixed),)+
                    $($required_lit => Ok(Self::$required),)+
                    $($unit_lit => Ok(Self::$unit),)+
                    _ => Ok(Self::Native(DesignNativeFeatureName(name.into()))),
                }
            }
        }

        impl From<DesignFeatureKind> for String {
            fn from(kind: DesignFeatureKind) -> Self { kind.as_str().to_owned() }
        }

        impl std::fmt::Display for DesignFeatureKind {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        /// Source family and its construction carrier. An independently decoded
        /// scope envelope can lack specialized construction data.
        #[derive(Debug, Clone, PartialEq)]
        pub enum DesignScopePayload {
            $($variant($payload),)+
            $($fixed($fixed_payload),)+
            $($required($required_payload),)+
            $($unit,)+
            /// Source name without a specialized construction grammar.
            Native(DesignNativeFeatureName),
        }

        /// Mutable construction fields with a fixed feature family.
        pub(crate) enum DesignScopePayloadMut<'a> {
            $($variant(&'a mut $payload),)+
            Other,
        }

        impl DesignScopePayload {
            fn fields_mut(&mut self) -> DesignScopePayloadMut<'_> {
                match self {
                    $(Self::$variant(value) => DesignScopePayloadMut::$variant(value),)+
                    _ => DesignScopePayloadMut::Other,
                }
            }
        }

        impl TryFrom<DesignFeatureKind> for DesignScopePayload {
            type Error = &'static str;
            fn try_from(kind: DesignFeatureKind) -> Result<Self, Self::Error> {
                match kind {
                    $(DesignFeatureKind::$variant => Ok(Self::$variant(Default::default())),)+
                    $(DesignFeatureKind::$fixed => Ok(Self::$fixed(Default::default())),)+
                    $(DesignFeatureKind::$required => Err(concat!($required_lit, " requires its operation")),)+
                    $(DesignFeatureKind::$unit => Ok(Self::$unit),)+
                    DesignFeatureKind::Native(name) => Ok(Self::Native(name)),
                }
            }
        }

        impl DesignScopePayload {
            pub(crate) fn kind(&self) -> DesignFeatureKind {
                match self {
                    $(Self::$variant(_) => DesignFeatureKind::$variant,)+
                    $(Self::$fixed(_) => DesignFeatureKind::$fixed,)+
                    $(Self::$required(_) => DesignFeatureKind::$required,)+
                    $(Self::$unit => DesignFeatureKind::$unit,)+
                    Self::Native(name) => DesignFeatureKind::Native(name.clone()),
                }
            }

            fn kind_name(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $lit,)+
                    $(Self::$fixed(_) => $fixed_lit,)+
                    $(Self::$required(_) => $required_lit,)+
                    $(Self::$unit => $unit_lit,)+
                    Self::Native(name) => &name.0,
                }
            }
        }
    };
}

design_feature_kinds! {
    data {
        Sketch => "Sketch": Option<DesignSketchEntityBinding>,
        Esquisse => "Esquisse": Option<DesignSketchEntityBinding>,
        Skizze => "Skizze": Option<DesignSketchEntityBinding>,
        Esboco => "Esboço": Option<DesignSketchEntityBinding>,
        Assemble => "Assemble": Option<DesignAssemblyAlignment>,
        AsBuilt => "As-built": Option<DesignAssemblyAlignment>,
        Extrude => "Extrude": Option<DesignExtrudeScope>,
        Extrusion => "Extrusion": Option<DesignExtrudeScope>,
        Extrusao => "Extrusão": Option<DesignExtrudeScope>,
        Fillet => "Fillet": Option<DesignFixedFilletParameters>,
        Conge => "Congé": Option<DesignFixedFilletParameters>,
        Abrundung => "Abrundung": Option<DesignFixedFilletParameters>,
        Arredondamento => "Arredondamento": Option<DesignFixedFilletParameters>,
        Chamfer => "Chamfer": Option<DesignFixedChamferParameters>,
        Chanfrein => "Chanfrein": Option<DesignFixedChamferParameters>,
        Combine => "Combine": Option<DesignCombineOperation>,
        Draft => "Draft": Option<DesignDraftOperation>,
        CPattern => "C-Pattern": Option<DesignCircularPatternConstruction>,
        CircularPattern => "Circular Pattern": Option<DesignCircularPatternConstruction>,
        ReseauC => "Réseau C": Option<DesignCircularPatternConstruction>,
        RPattern => "R-Pattern": Option<DesignRectangularPatternConstruction>,
        RectangularPattern => "Rectangular Pattern": Option<DesignRectangularPatternConstruction>,
        Mirror => "Mirror": Option<DesignMirrorConstruction>,
        SymetrieMiroir => "Symétrie miroir": Option<DesignMirrorConstruction>,
        Move => "Move": Option<DesignMoveOperation>,
        OffsetFaces => "OffsetFaces": Option<DesignOffsetFacesOperation>,
        DecalerLesFaces => "DécalerLesFaces": Option<DesignOffsetFacesOperation>,
        Shell => "Shell": Option<DesignShellOperation>,
        Schale => "Schale": Option<DesignShellOperation>,
        Thicken => "Thicken": Option<DesignThickenOperation>,
        SpirePrimitive => "SpirePrimitive": Option<DesignCoilScope>,
        CoilPrimitive => "CoilPrimitive": Option<DesignCoilScope>,
        Sweep => "Sweep": Option<DesignSweepScope>,
        SurfacePatch => "SurfacePatch": Vec<DesignSurfacePatchBoundary>,
        SurfaceExtend => "SurfaceExtend": Option<DesignSurfaceExtendOperation>,
        SurfaceOffset => "SurfaceOffset": Option<DesignSurfaceOffsetOperation>,
        Hole => "Hole": Option<DesignHoleConstruction>,
        Scale => "Scale": Option<DesignScaleOperation>,
        Massstab => "Maßstab": Option<DesignScaleOperation>,
        Thread => "Thread": Option<DesignThreadConstruction>,
        EdgeFlange => "EdgeFlange": Option<DesignEdgeFlangeOperation>,
        Hem => "Hem": Option<DesignHemOperation>,
        BaseFlange => "BaseFlange": Option<DesignBaseFlangeScope>,
        ComponentInsert => "Component Insert": Option<DesignComponentInsertConstruction>,
        CopyPaste => "CopyPaste": Option<DesignCopyPasteComponentOperation>,
        JointOrigin => "JointOrigin": Option<DesignJointOriginTransform>,
        WorkPlane => "WorkPlane": Option<DesignWorkPlaneTransform>,
        WorkAxis => "WorkAxis": Option<DesignWorkAxisConstruction>,
        WorkPoint => "WorkPoint": Option<DesignWorkPointConstruction>,
        DerivedInstance => "DerivedInstance": Option<DesignDerivedInstanceConstruction>,
        BaseFeature => "Base Feature": Option<DesignBaseFeatureConstruction>,
        CopyPasteBodies => "CopyPasteBodies": Option<DesignCopyPasteBodiesOperation>,
    }
    fixed {
        Revolve => "Revolve": Option<DesignRevolveConstruction>,
        Loft => "Loft": Option<DesignLoftConstruction>,
        Pipe => "Pipe": Option<DesignPipeConstruction>,
        SpherePrimitive => "SpherePrimitive": Option<DesignSpherePrimitive>,
        TorusPrimitive => "TorusPrimitive": Option<DesignTorusPrimitive>,
        BoxPrimitive => "BoxPrimitive": Option<DesignBoxPrimitive>,
        CylinderPrimitive => "CylinderPrimitive": Option<DesignCylinderPrimitive>,
    }
    required {
        SurfaceStitch => "SurfaceStitch": DesignSurfaceStitchOperation,
        SurfaceRuled => "SurfaceRuled": DesignRuledSurfaceOperation,
    }
    names {
        ReplaceFace => "ReplaceFace",
        SurfaceTrim => "SurfaceTrim",
        BoundaryFill => "BoundaryFill",
        Split => "Split",
        Canvas => "Canvas",
        Decal => "Decal",
        BaseMeshFeature => "Base Mesh Feature",
        CustomFeature => "CustomFeature",
        Form => "Form",
        SplitFace => "SplitFace",
        DeleteFace => "DeleteFace",
        SurfaceDeleteFace => "SurfaceDeleteFace",
        RemoveBody => "RemoveBody",
        Face => "Face",
    }
}

/// Rejected CADIR payload that names more than one family or disagrees with `kind`.
#[derive(Debug)]
pub(crate) struct DesignParameterScopePayloadError(String);

impl std::fmt::Display for DesignParameterScopePayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DesignParameterScopePayloadError {}

/// Indexed sketch or construction-operation record that scopes parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignParameterScopeSerde",
    into = "DesignParameterScopeSerde"
)]
pub struct DesignParameterScope {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    frame_length: u64,
    /// Byte offset of the kind's UTF-16LE code units.
    kind_offset: u64,
    /// One-based ordinal among scopes of the same feature family.
    pub feature_ordinal: std::num::NonZeroU32,
    /// Byte offset of `feature_ordinal`.
    feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history_state_id: Option<i64>,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    reference_members: ReferenceRun<u32>,
    /// Family-specific construction records.
    payload: DesignScopePayload,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unclosed_construction_operand_groups: Vec<u32>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the paired indexed record header.
    paired_byte_offset: u64,
}

/// Unadmitted parameter-scope fields.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignParameterScopeDraft {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Byte offset of the kind's UTF-16LE code units.
    pub kind_offset: u64,
    /// One-based ordinal among scopes of the same feature family.
    pub feature_ordinal: std::num::NonZeroU32,
    /// Byte offset of `feature_ordinal`.
    pub feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    pub history_state_id: Option<i64>,
    /// ASM delta-state identity immediately preceding this scope, when active.
    pub previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    pub previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    pub reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    pub reference_members: ReferenceRun<u32>,
    /// Family-specific construction records.
    pub payload: DesignScopePayload,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    pub unclosed_construction_operand_groups: Vec<u32>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

/// Wire form of [`DesignParameterScope`] with the historical flat field set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignParameterScopeSerde {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the primary indexed record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Shared logical record identity.
    pub record_index: u32,
    /// Byte length from the primary header to the paired header.
    pub frame_length: u64,
    /// Source feature-family name.
    pub kind: DesignFeatureKind,
    /// Byte offset of the kind's UTF-16LE code units.
    pub kind_offset: u64,
    /// Extrude prologue, fixed parameters, and profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "extrude_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub extrude: Option<DesignExtrudeScope>,
    /// Coil discriminators, placement, and transform.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "coil_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub coil: Option<DesignCoilScope>,
    /// One-based ordinal among scopes of the same feature family.
    pub feature_ordinal: u32,
    /// Byte offset of `feature_ordinal`.
    pub feature_ordinal_offset: u64,
    /// ASM delta-state identity produced by this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_state_id: Option<i64>,
    /// Byte offset of the encoded history-state identity or null sentinel.
    pub history_state_id_offset: u64,
    /// ASM delta-state identity immediately preceding this scope, when active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_history_state_id: Option<i64>,
    /// Byte offset of the encoded preceding-state identity, when present.
    #[serde(
        default,
        serialize_with = "serialize_absent_u64_offset",
        deserialize_with = "deserialize_absent_u64_offset"
    )]
    pub previous_history_state_id_offset: Option<u64>,
    /// Byte offset of the ordered reference-table count.
    pub reference_count_offset: u64,
    /// Ordered indexed-record references carried by the scope.
    pub reference_members: Vec<u32>,
    /// Byte offsets parallel to `reference_members`.
    pub reference_member_offsets: Vec<u64>,
    /// Exact solid-primitive construction carried by this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_primitive: Option<DesignSolidPrimitive>,
    /// Exact fixed-form construction carried by a direct-face scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_face_operation: Option<DesignDirectFaceOperation>,
    /// Exact rigid transform carried by a Move scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_operation: Option<DesignMoveOperation>,
    /// Exact uniform body-scale construction carried by a Scale scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_operation: Option<DesignScaleOperation>,
    /// Exact tolerance and setting-record references carried by a `SurfaceStitch` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_stitch_operation: Option<DesignSurfaceStitchOperation>,
    /// Exact distance, method, and boundary records carried by a `SurfaceExtend` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_extend_operation: Option<DesignSurfaceExtendOperation>,
    /// Exact distance and boundary records carried by a `SurfaceOffset` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_offset_operation: Option<DesignSurfaceOffsetOperation>,
    /// Exact mode, parameter, and selection records carried by a `SurfaceRuled` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruled_surface_operation: Option<DesignRuledSurfaceOperation>,
    /// `BaseFlange` operation and sketch profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "base_flange_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub base_flange: Option<DesignBaseFlangeScope>,
    /// Per-boundary-component settings carried by a `SurfacePatch` scope, in
    /// scope reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_patch_boundaries: Vec<DesignSurfacePatchBoundary>,
    /// Exact edge, parameter, and settings records carried by an `EdgeFlange` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_flange_operation: Option<DesignEdgeFlangeOperation>,
    /// Exact edge, parameter, and settings records carried by a `Hem` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hem_operation: Option<DesignHemOperation>,

    /// Exact fixed scalar lanes carried by a Fillet scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_fillet_parameters: Option<DesignFixedFilletParameters>,
    /// Exact fixed scalar lane carried by an equal-distance Chamfer scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_chamfer_parameters: Option<DesignFixedChamferParameters>,
    /// Path-feature construction and Sweep sketch profile.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "path_feature_scope_is_absent")]
    #[serde(deserialize_with = "deserialize_flattened_scope")]
    pub path_feature: Option<DesignPathFeatureWire>,
    /// Exact Boolean construction carried by a `Combine` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combine_operation: Option<DesignCombineOperation>,
    /// Exact form and size construction carried by a `Thread` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_construction: Option<DesignThreadConstruction>,
    /// Exact signed-angle construction carried by a `Draft` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_operation: Option<DesignDraftOperation>,
    /// Exact construction carried by a circular-pattern scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circular_pattern_construction: Option<DesignCircularPatternConstruction>,
    /// Exact scalar lanes carried by a rectangular-pattern scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rectangular_pattern_construction: Option<DesignRectangularPatternConstruction>,
    /// Exact alignment scalars carried by an `Assemble` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_alignment: Option<DesignAssemblyAlignment>,
    /// Exact external-occurrence construction carried by a `Component Insert` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_insert_construction: Option<DesignComponentInsertConstruction>,
    /// Exact local-occurrence construction carried by a `DerivedInstance` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_instance_construction: Option<DesignDerivedInstanceConstruction>,
    /// Exact local-component construction carried by a legacy `CopyPaste` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_paste_component_operation: Option<DesignCopyPasteComponentOperation>,
    /// Exact construction carried by a Mirror scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_construction: Option<DesignMirrorConstruction>,
    /// Exact source-to-copy body mapping carried by a `CopyPasteBodies` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_paste_bodies_operation: Option<DesignCopyPasteBodiesOperation>,
    /// Exact result-body references carried by a `Base Feature` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_feature_construction: Option<DesignBaseFeatureConstruction>,
    /// Exact row-major local-to-model frame carried by a `WorkPlane` scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_work_plane_frame")]
    pub work_plane_frame: Option<DesignWorkPlaneTransform>,
    /// Exact two-point construction carried by a `WorkAxis` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_axis_construction: Option<DesignWorkAxisConstruction>,
    /// Exact row-major local-to-model frame owned by a `JointOrigin` scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_joint_origin_frame")]
    pub joint_origin_frame: Option<DesignJointOriginTransform>,

    /// Exact solved construction carried by a `WorkPoint` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_point_construction: Option<DesignWorkPointConstruction>,
    /// Reference members whose records open a construction-operand group the
    /// group grammar does not close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unclosed_construction_operand_groups: Vec<u32>,
    /// Exact point-and-direction construction carried by a `Hole` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hole_construction: Option<DesignHoleConstruction>,

    /// Sketch-module entity bound to this sketch scope.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_sketch_entity")]
    pub sketch_entity: Option<DesignSketchEntityBinding>,
    /// Per-file dynamic class tag of the paired header.
    pub paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    pub paired_byte_offset: u64,
}

// Deserialize the payload itself: flattened Option<T> suppresses T's errors.
fn deserialize_flattened_scope<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct WorkPlaneFrameWire {
    work_plane_transform: Option<SketchPlacementMatrix>,
    work_plane_transform_offset: Option<u64>,
    work_plane_reference: Option<u32>,
    work_plane_reference_offset: Option<u64>,
    work_plane_construction: Option<DesignWorkPlaneConstruction>,
}

fn deserialize_work_plane_frame<'de, D>(
    deserializer: D,
) -> Result<Option<DesignWorkPlaneTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = WorkPlaneFrameWire::deserialize(deserializer)?;
    let reference = match (wire.work_plane_reference, wire.work_plane_reference_offset) {
        (None, None) => None,
        (Some(work_plane_reference), Some(work_plane_reference_offset)) => {
            Some(DesignWorkPlaneReference {
                work_plane_reference,
                work_plane_reference_offset,
            })
        }
        _ => {
            return Err(serde::de::Error::custom(
                "work_plane_reference and work_plane_reference_offset must occur together",
            ))
        }
    };
    match (wire.work_plane_transform, wire.work_plane_transform_offset) {
        (None, None) if reference.is_none() && wire.work_plane_construction.is_none() => Ok(None),
        (Some(work_plane_transform), Some(work_plane_transform_offset)) => Ok(Some(DesignWorkPlaneTransform {
            work_plane_transform,
            work_plane_transform_offset,
            reference,
            work_plane_construction: wire.work_plane_construction,
        })),
        _ => Err(serde::de::Error::custom("work_plane_transform and work_plane_transform_offset are required for work_plane frame data")),
    }
}

impl<'de> Deserialize<'de> for DesignWorkPlaneTransform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_work_plane_frame(deserializer)?
            .ok_or_else(|| serde::de::Error::missing_field("work_plane_transform"))
    }
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct JointOriginFrameWire {
    joint_origin_transform: Option<SketchPlacementMatrix>,
    joint_origin_transform_offset: Option<u64>,
    joint_origin_reference: Option<u32>,
    joint_origin_reference_offset: Option<u64>,
}

fn deserialize_joint_origin_frame<'de, D>(
    deserializer: D,
) -> Result<Option<DesignJointOriginTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = JointOriginFrameWire::deserialize(deserializer)?;
    let reference = match (
        wire.joint_origin_reference,
        wire.joint_origin_reference_offset,
    ) {
        (None, None) => None,
        (Some(joint_origin_reference), Some(joint_origin_reference_offset)) => {
            Some(DesignJointOriginReference {
                joint_origin_reference,
                joint_origin_reference_offset,
            })
        }
        _ => {
            return Err(serde::de::Error::custom(
                "joint_origin_reference and joint_origin_reference_offset must occur together",
            ))
        }
    };
    match (wire.joint_origin_transform, wire.joint_origin_transform_offset) {
        (None, None) if reference.is_none() => Ok(None),
        (Some(joint_origin_transform), Some(joint_origin_transform_offset)) => Ok(Some(DesignJointOriginTransform {
            joint_origin_transform,
            joint_origin_transform_offset,
            reference,
        })),
        _ => Err(serde::de::Error::custom("joint_origin_transform and joint_origin_transform_offset are required for joint_origin frame data")),
    }
}

impl<'de> Deserialize<'de> for DesignJointOriginTransform {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_joint_origin_frame(deserializer)?
            .ok_or_else(|| serde::de::Error::missing_field("joint_origin_transform"))
    }
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct SketchEntityWire {
    entity_id: Option<String>,
    entity_suffix: Option<u64>,
    entity_reference_offset: Option<u64>,
}

fn deserialize_sketch_entity<'de, D>(
    deserializer: D,
) -> Result<Option<DesignSketchEntityBinding>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = SketchEntityWire::deserialize(deserializer)?;
    match (
        wire.entity_id,
        wire.entity_suffix,
        wire.entity_reference_offset,
    ) {
        (None, None, None) => Ok(None),
        (Some(entity_id), Some(entity_suffix), Some(entity_reference_offset)) => {
            DesignSketchEntityBinding::try_from(DesignSketchEntityBindingWire {
                entity_id,
                entity_suffix,
                entity_reference_offset,
            })
            .map(Some)
            .map_err(serde::de::Error::custom)
        }
        _ => Err(serde::de::Error::custom(
            "entity_id, entity_suffix, and entity_reference_offset must occur together",
        )),
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
fn base_flange_scope_is_absent(base_flange: &Option<DesignBaseFlangeScope>) -> bool {
    match base_flange {
        None => true,
        Some(base_flange) => {
            base_flange.base_flange_operation.is_none() && base_flange.base_flange_profile.is_none()
        }
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
fn coil_scope_is_absent(coil: &Option<DesignCoilScope>) -> bool {
    match coil {
        None => true,
        Some(coil) => {
            coil.coil_operation.is_none()
                && coil.coil_extent.is_none()
                && coil.coil_section.is_none()
                && coil.coil_section_placement.is_none()
                && coil.coil_clockwise.is_none()
                && coil.coil_placement.is_none()
                && coil.coil_transform.is_none()
        }
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
fn extrude_scope_is_absent(extrude: &Option<DesignExtrudeScope>) -> bool {
    match extrude {
        None => true,
        Some(extrude) => {
            extrude.extrude_prologue.is_none()
                && extrude.fixed_extrude_parameters.is_none()
                && extrude.extrude_profile.is_none()
        }
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
fn path_feature_scope_is_absent(path_feature: &Option<DesignPathFeatureWire>) -> bool {
    match path_feature {
        None => true,
        Some(path_feature) => {
            path_feature.path_feature_construction.is_none() && path_feature.sweep_profile.is_none()
        }
    }
}

/// BaseFlange-specific records carried by a `BaseFlange` parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DesignBaseFlangeScope {
    /// Exact profile and thickness records carried by a `BaseFlange` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_flange_operation: Option<DesignBaseFlangeOperation>,
    /// Sketch-profile operand carried by a `BaseFlange` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_flange_profile: Option<DesignSketchProfileOperand>,
}

/// Extrude-specific records carried by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DesignExtrudeScope {
    /// Extrude fixed prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_prologue: Option<DesignExtrudePrologue>,
    /// Exact fixed scalar lanes carried by an Extrude scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_extrude_parameters: Option<DesignFixedExtrudeParameters>,
    /// Profile operand carried by an Extrude scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extrude_profile: Option<DesignSketchProfileOperand>,
}

/// Sweep construction and its independently decoded profile operand.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DesignSweepScope {
    pub construction: Option<DesignSweepConstruction>,
    pub sweep_profile: Option<DesignSketchProfileOperand>,
}

impl From<DesignPathFeatureConstruction> for DesignScopePayload {
    fn from(value: DesignPathFeatureConstruction) -> Self {
        match value {
            DesignPathFeatureConstruction::Revolve(value) => Self::Revolve(Some(value)),
            DesignPathFeatureConstruction::Loft(value) => Self::Loft(Some(value)),
            DesignPathFeatureConstruction::Pipe(value) => Self::Pipe(Some(value)),
            DesignPathFeatureConstruction::Sweep(value) => Self::Sweep(Some(DesignSweepScope {
                construction: Some(value),
                sweep_profile: None,
            })),
        }
    }
}

/// Flat wire fields for path construction and the Sweep profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct DesignPathFeatureWire {
    /// Exact fixed construction carried by a Loft, Sweep, Revolve, or Pipe scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_feature_construction: Option<DesignPathFeatureConstruction>,
    /// Sketch-profile operand carried by a `Sweep` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_profile: Option<DesignSketchProfileOperand>,
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(try_from = "DesignCoilScopeWire", into = "DesignCoilScopeWire")]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
pub struct DesignCoilScope {
    pub coil_operation: Option<RecordedValue<DesignExtrudeOperation>>,
    pub coil_extent: Option<MaybeRecordedValue<DesignCoilExtent>>,
    pub coil_section: Option<MaybeRecordedValue<DesignCoilSection>>,
    pub coil_section_placement: Option<MaybeRecordedValue<DesignCoilSectionPlacement>>,
    pub coil_clockwise: Option<MaybeRecordedValue<bool>>,
    pub coil_placement: Option<DesignCoilPlacement>,
    pub coil_transform: Option<DesignCoilTransform>,
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct DesignCoilScopeWire {
    /// Coil result operation from the fixed scope prologue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_operation: Option<DesignExtrudeOperation>,
    /// Byte offset of the Coil operation enum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_operation_offset: Option<u64>,
    /// Coil driving-dimension mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_extent: Option<DesignCoilExtent>,
    /// Byte offset of the Coil mode enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_extent_offset: Option<u64>,
    /// Generated Coil section family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section: Option<DesignCoilSection>,
    /// Byte offset of the Coil section enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section_offset: Option<u64>,
    /// Radial placement of the generated Coil section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section_placement: Option<DesignCoilSectionPlacement>,
    /// Byte offset of the Coil section-placement enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_section_placement_offset: Option<u64>,
    /// Whether Coil angular travel is clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_clockwise: Option<bool>,
    /// Byte offset of the Coil direction enum, when the form stores one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_clockwise_offset: Option<u64>,
    /// Exact placement construction carried by a compact Coil scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_placement: Option<DesignCoilPlacement>,
    /// Direct rigid placement carried by the long ten-reference Coil form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_transform: Option<DesignCoilTransform>,
}

impl TryFrom<DesignCoilScopeWire> for DesignCoilScope {
    type Error = String;
    fn try_from(wire: DesignCoilScopeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            coil_operation: RecordedValue::from_wire(
                wire.coil_operation,
                wire.coil_operation_offset,
                "coil_operation",
            )?,
            coil_extent: MaybeRecordedValue::from_wire(
                wire.coil_extent,
                wire.coil_extent_offset,
                "coil_extent",
            )?,
            coil_section: MaybeRecordedValue::from_wire(
                wire.coil_section,
                wire.coil_section_offset,
                "coil_section",
            )?,
            coil_section_placement: MaybeRecordedValue::from_wire(
                wire.coil_section_placement,
                wire.coil_section_placement_offset,
                "coil_section_placement",
            )?,
            coil_clockwise: MaybeRecordedValue::from_wire(
                wire.coil_clockwise,
                wire.coil_clockwise_offset,
                "coil_clockwise",
            )?,
            coil_placement: wire.coil_placement,
            coil_transform: wire.coil_transform,
        })
    }
}

impl From<DesignCoilScope> for DesignCoilScopeWire {
    fn from(value: DesignCoilScope) -> Self {
        Self {
            coil_operation: value.coil_operation.map(|field| field.value),
            coil_operation_offset: value.coil_operation.map(|field| field.offset),
            coil_extent: value.coil_extent.map(|field| field.value()),
            coil_extent_offset: value.coil_extent.and_then(|field| field.offset()),
            coil_section: value.coil_section.map(|field| field.value()),
            coil_section_offset: value.coil_section.and_then(|field| field.offset()),
            coil_section_placement: value.coil_section_placement.map(|field| field.value()),
            coil_section_placement_offset: value
                .coil_section_placement
                .and_then(|field| field.offset()),
            coil_clockwise: value.coil_clockwise.map(|field| field.value()),
            coil_clockwise_offset: value.coil_clockwise.and_then(|field| field.offset()),
            coil_placement: value.coil_placement,
            coil_transform: value.coil_transform,
        }
    }
}

/// Sketch-module entity named by a sketch parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchEntityBindingWire",
    into = "DesignSketchEntityBindingWire"
)]
pub struct DesignSketchEntityBinding {
    /// Full Design entity id of a sketch scope.
    pub entity_id: DesignEntityId,
    /// Byte offset of the sketch entity suffix.
    pub entity_reference_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct DesignSketchEntityBindingWire {
    /// Full Design entity id of a sketch scope.
    entity_id: String,
    /// Numeric suffix of `entity_id`.
    entity_suffix: u64,
    /// Byte offset of the sketch entity suffix.
    entity_reference_offset: u64,
}

impl TryFrom<DesignSketchEntityBindingWire> for DesignSketchEntityBinding {
    type Error = String;

    fn try_from(wire: DesignSketchEntityBindingWire) -> Result<Self, Self::Error> {
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        Ok(Self {
            entity_id,
            entity_reference_offset: wire.entity_reference_offset,
        })
    }
}

impl From<DesignSketchEntityBinding> for DesignSketchEntityBindingWire {
    fn from(value: DesignSketchEntityBinding) -> Self {
        let entity_suffix = value.entity_id.suffix();
        Self {
            entity_id: value.entity_id.0,
            entity_suffix,
            entity_reference_offset: value.entity_reference_offset,
        }
    }
}

/// Explicit 16-f64 frame carried by a `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignWorkPlaneTransform {
    /// Exact row-major local-to-model frame.
    pub work_plane_transform: SketchPlacementMatrix,
    /// Byte offset of the explicit 16-f64 matrix.
    pub work_plane_transform_offset: u64,
    /// Construction record referenced by the frame, when present.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<DesignWorkPlaneReference>,
    /// Exact construction rule carried by this `WorkPlane` frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_plane_construction: Option<DesignWorkPlaneConstruction>,
}

/// Construction record named by a `WorkPlane` frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignWorkPlaneReference {
    /// Construction record referenced by the `WorkPlane` frame.
    pub work_plane_reference: u32,
    /// Byte offset of the `WorkPlane` construction reference.
    pub work_plane_reference_offset: u64,
}

/// Explicit 16-f64 frame carried by a `JointOrigin` scope.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignJointOriginTransform {
    /// Exact row-major local-to-model frame.
    pub joint_origin_transform: SketchPlacementMatrix,
    /// Byte offset of the explicit 16-f64 matrix.
    pub joint_origin_transform_offset: u64,
    /// Construction record referenced by the frame, when present.
    #[serde(flatten)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<DesignJointOriginReference>,
}

/// Construction record named by a `JointOrigin` frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignJointOriginReference {
    /// Construction record referenced by the `JointOrigin` frame.
    pub joint_origin_reference: u32,
    /// Byte offset of the `JointOrigin` construction reference.
    pub joint_origin_reference_offset: u64,
}

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

/// Fixed construction carried by a planar sheet-metal `BaseFlange` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignBaseFlangeOperation {
    /// Positive sheet thickness in centimetres.
    pub thickness: DesignPositiveScalar,
    /// Byte offset of `thickness`.
    pub thickness_offset: u64,
    /// Counted sketch-profile operand group.
    pub profile_group_record_index: u32,
    /// Sketch-profile record contained by the profile group.
    pub profile_record_index: u32,
    /// Indexed thickness-construction record.
    pub thickness_record_index: u32,
    /// Indexed operation-settings record.
    pub settings_record_index: u32,
}

/// Bend position used by sheet-metal edge operations.
///
/// The position places the bend region against the selected edge: `Outside` and
/// `Inside` put the bend beyond and within the source face boundary, `Adjacent`
/// starts it at the boundary, and `TangentToSide` makes it tangent to the side
/// reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignBendPosition {
    /// The bend lies outside the selected edge.
    Outside,
    /// The bend lies inside the selected edge.
    Inside,
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the side reference plane.
    TangentToSide,
    /// A serialized value whose bend-position meaning is not settled.
    Unknown(u32),
}

impl DesignBendPosition {
    /// Decode the serialized bend-position discriminator without discarding unknown values.
    #[must_use]
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Outside,
            2 => Self::Inside,
            3 => Self::Adjacent,
            4 => Self::TangentToSide,
            code => Self::Unknown(code),
        }
    }
}

/// Face pair an `EdgeFlange` height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignSheetMetalHeightDatum {
    /// The height is measured from the inner faces of the sheet.
    InnerFaces,
    /// The height is measured from the outer faces of the sheet.
    OuterFaces,
    /// A serialized value whose height-datum meaning is not settled.
    Unknown(u32),
}

impl DesignSheetMetalHeightDatum {
    /// Decode the serialized height-datum discriminator without discarding unknown values.
    #[must_use]
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::InnerFaces,
            2 => Self::OuterFaces,
            code => Self::Unknown(code),
        }
    }
}

/// Extent of an `EdgeFlange` along its selected edge.
///
/// Ordinary forms derive the mode from the count of width-distance parameter
/// owners in the ordered reference table. Classed forms can carry a distinct
/// explicit mode when that count has per-edge meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignEdgeWidthMode {
    /// The flange spans the complete selected edge and adds no width owner.
    FullEdge,
    /// The flange is centred on the edge and adds one width owner.
    Symmetric,
    /// The flange is measured from each end and adds two width owners.
    TwoSides,
    /// The fixed section carries one symmetric-width owner per selected edge.
    ///
    /// Neutral projection can collapse these owners to one symmetric width only
    /// when their stored values agree. Distinct values remain source-native.
    SymmetricPerEdge,
    /// The fixed section carries one `EdgeWidth_1`/`EdgeWidth_2` pair per
    /// selected edge. The edge-local orientation is not part of the neutral law.
    TwoSidesPerEdge,
}

/// A selected flange edge and its width parameter owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignFlangeEdgeWidth<T> {
    pub edge: DesignEdgeFlangeEdge,
    pub owners: T,
}

/// Flange extent, with width owners attached to the edges they describe.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignEdgeFlangeShape {
    FullEdge {
        edges: Vec<DesignEdgeFlangeEdge>,
        height: DesignEdgeFlangeHeightExtent,
    },
    Symmetric {
        edges: Vec<DesignEdgeFlangeEdge>,
        owner: u32,
    },
    TwoSides {
        edges: Vec<DesignEdgeFlangeEdge>,
        owners: [u32; 2],
    },
    SymmetricPerEdge(Vec<DesignFlangeEdgeWidth<u32>>),
    TwoSidesPerEdge {
        edges: Vec<DesignFlangeEdgeWidth<[u32; 2]>>,
        source: DesignEdgeFlangeWidthParameterSource,
    },
}

impl DesignEdgeFlangeShape {
    // The tuple carries one coupled result; a separate alias would add no invariant.
    #[allow(clippy::type_complexity)]
    pub(crate) fn edges(&self) -> impl Iterator<Item = &DesignEdgeFlangeEdge> {
        let (shared, symmetric, two_sided): (
            &[DesignEdgeFlangeEdge],
            &[DesignFlangeEdgeWidth<u32>],
            &[DesignFlangeEdgeWidth<[u32; 2]>],
        ) = match self {
            Self::FullEdge { edges, .. }
            | Self::Symmetric { edges, .. }
            | Self::TwoSides { edges, .. } => (edges, &[], &[]),
            Self::SymmetricPerEdge(edges) => (&[], edges, &[]),
            Self::TwoSidesPerEdge { edges, .. } => (&[], &[], edges),
        };
        shared
            .iter()
            .chain(symmetric.iter().map(|row| &row.edge))
            .chain(two_sided.iter().map(|row| &row.edge))
    }

    pub(crate) fn mode(&self) -> DesignEdgeWidthMode {
        match self {
            Self::FullEdge { .. } => DesignEdgeWidthMode::FullEdge,
            Self::Symmetric { .. } => DesignEdgeWidthMode::Symmetric,
            Self::TwoSides { .. } => DesignEdgeWidthMode::TwoSides,
            Self::SymmetricPerEdge(_) => DesignEdgeWidthMode::SymmetricPerEdge,
            Self::TwoSidesPerEdge { .. } => DesignEdgeWidthMode::TwoSidesPerEdge,
        }
    }

    pub(crate) fn height(&self) -> DesignEdgeFlangeHeightExtent {
        match self {
            Self::FullEdge { height, .. } => *height,
            _ => DesignEdgeFlangeHeightExtent::Distance,
        }
    }

    pub(crate) fn source(&self) -> DesignEdgeFlangeWidthParameterSource {
        match self {
            Self::TwoSidesPerEdge { source, .. } => *source,
            _ => DesignEdgeFlangeWidthParameterSource::EdgeWidth,
        }
    }

    // The tuple carries one coupled result; a separate alias would add no invariant.
    #[allow(clippy::type_complexity)]
    pub(crate) fn owner_indices(&self) -> impl Iterator<Item = &u32> {
        let (shared, symmetric, two_sided): (
            &[u32],
            &[DesignFlangeEdgeWidth<u32>],
            &[DesignFlangeEdgeWidth<[u32; 2]>],
        ) = match self {
            Self::FullEdge { .. } => (&[], &[], &[]),
            Self::Symmetric { owner, .. } => (std::slice::from_ref(owner), &[], &[]),
            Self::TwoSides { owners, .. } => (owners, &[], &[]),
            Self::SymmetricPerEdge(edges) => (&[], edges, &[]),
            Self::TwoSidesPerEdge { edges, .. } => (&[], &[], edges),
        };
        shared
            .iter()
            .chain(symmetric.iter().map(|row| &row.owners))
            .chain(two_sided.iter().flat_map(|row| &row.owners))
    }

    pub(crate) fn from_wire(
        edges: Vec<DesignEdgeFlangeEdge>,
        width_mode: Option<DesignEdgeWidthMode>,
        owners: Vec<u32>,
        owners_by_edge: Vec<[u32; 2]>,
        source: DesignEdgeFlangeWidthParameterSource,
        height: DesignEdgeFlangeHeightExtent,
    ) -> Result<Self, String> {
        let mode = width_mode.unwrap_or(match owners.len() {
            0 => DesignEdgeWidthMode::FullEdge,
            1 => DesignEdgeWidthMode::Symmetric,
            _ => DesignEdgeWidthMode::TwoSides,
        });
        if source == DesignEdgeFlangeWidthParameterSource::EdgeOffset
            && mode != DesignEdgeWidthMode::TwoSidesPerEdge
        {
            return Err(
                "width_parameter_source edge_offset requires width_mode two_sides_per_edge".into(),
            );
        }
        if !matches!(height, DesignEdgeFlangeHeightExtent::Distance)
            && mode != DesignEdgeWidthMode::FullEdge
        {
            return Err("height_extent to_object requires width_mode full_edge".into());
        }
        match mode {
            DesignEdgeWidthMode::FullEdge if owners.is_empty() && owners_by_edge.is_empty() => Ok(Self::FullEdge { edges, height }),
            DesignEdgeWidthMode::Symmetric if owners.len() == 1 && owners_by_edge.is_empty() => Ok(Self::Symmetric { edges, owner: owners[0] }),
            DesignEdgeWidthMode::TwoSides if owners.len() == 2 && owners_by_edge.is_empty() => Ok(Self::TwoSides { edges, owners: [owners[0], owners[1]] }),
            DesignEdgeWidthMode::SymmetricPerEdge if !owners.is_empty() && owners.len() == edges.len() && owners_by_edge.is_empty() => {
                Ok(Self::SymmetricPerEdge(edges.into_iter().zip(owners).map(|(edge, owners)| DesignFlangeEdgeWidth { edge, owners }).collect()))
            }
            DesignEdgeWidthMode::TwoSidesPerEdge if !owners_by_edge.is_empty() && owners_by_edge.len() == edges.len()
                && owners.iter().eq(owners_by_edge.iter().flatten()) => {
                Ok(Self::TwoSidesPerEdge { edges: edges.into_iter().zip(owners_by_edge).map(|(edge, owners)| DesignFlangeEdgeWidth { edge, owners }).collect(), source })
            }
            _ => Err("width_mode, width_distance_owner_record_indices, and width_distance_owner_record_indices_by_edge must match the selected edges".into()),
        }
    }
}

/// Parameter source used by a typed `EdgeFlange` width law.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignEdgeFlangeWidthParameterSource {
    /// Width parameters use the ordinary positive `EdgeWidth` source kinds.
    #[default]
    EdgeWidth,
    /// Legacy edge-end parameters use signed `EdgeOffset` source kinds.
    EdgeOffset,
}

impl TryFrom<DesignParameterScopeSerde> for DesignParameterScope {
    type Error = DesignParameterScopePayloadError;

    fn try_from(mut wire: DesignParameterScopeSerde) -> Result<Self, Self::Error> {
        if extrude_scope_is_absent(&wire.extrude) {
            wire.extrude = None;
        }
        if coil_scope_is_absent(&wire.coil) {
            wire.coil = None;
        }
        if base_flange_scope_is_absent(&wire.base_flange) {
            wire.base_flange = None;
        }
        if path_feature_scope_is_absent(&wire.path_feature) {
            wire.path_feature = None;
        }
        let mut present = Vec::new();
        if wire.extrude.is_some() {
            present.push("extrude");
        }
        if wire.coil.is_some() {
            present.push("coil");
        }
        if wire.base_flange.is_some() {
            present.push("base_flange");
        }
        if wire.path_feature.is_some() {
            present.push("path_feature");
        }
        if wire.work_plane_frame.is_some() {
            present.push("work_plane_frame");
        }
        if wire.joint_origin_frame.is_some() {
            present.push("joint_origin_frame");
        }
        if wire.sketch_entity.is_some() {
            present.push("sketch_entity");
        }
        if wire.solid_primitive.is_some() {
            present.push("solid_primitive");
        }
        if wire.direct_face_operation.is_some() {
            present.push("direct_face_operation");
        }
        if wire.move_operation.is_some() {
            present.push("move_operation");
        }
        if wire.scale_operation.is_some() {
            present.push("scale_operation");
        }
        if wire.surface_stitch_operation.is_some() {
            present.push("surface_stitch_operation");
        }
        if wire.surface_extend_operation.is_some() {
            present.push("surface_extend_operation");
        }
        if wire.surface_offset_operation.is_some() {
            present.push("surface_offset_operation");
        }
        if wire.ruled_surface_operation.is_some() {
            present.push("ruled_surface_operation");
        }
        if !wire.surface_patch_boundaries.is_empty() {
            present.push("surface_patch_boundaries");
        }
        if wire.edge_flange_operation.is_some() {
            present.push("edge_flange_operation");
        }
        if wire.hem_operation.is_some() {
            present.push("hem_operation");
        }
        if wire.fixed_fillet_parameters.is_some() {
            present.push("fixed_fillet_parameters");
        }
        if wire.fixed_chamfer_parameters.is_some() {
            present.push("fixed_chamfer_parameters");
        }
        if wire.combine_operation.is_some() {
            present.push("combine_operation");
        }
        if wire.thread_construction.is_some() {
            present.push("thread_construction");
        }
        if wire.draft_operation.is_some() {
            present.push("draft_operation");
        }
        if wire.circular_pattern_construction.is_some() {
            present.push("circular_pattern_construction");
        }
        if wire.rectangular_pattern_construction.is_some() {
            present.push("rectangular_pattern_construction");
        }
        if wire.assembly_alignment.is_some() {
            present.push("assembly_alignment");
        }
        if wire.component_insert_construction.is_some() {
            present.push("component_insert_construction");
        }
        if wire.derived_instance_construction.is_some() {
            present.push("derived_instance_construction");
        }
        if wire.copy_paste_component_operation.is_some() {
            present.push("copy_paste_component_operation");
        }
        if wire.copy_paste_bodies_operation.is_some() {
            present.push("copy_paste_bodies_operation");
        }
        if wire.mirror_construction.is_some() {
            present.push("mirror_construction");
        }
        if wire.base_feature_construction.is_some() {
            present.push("base_feature_construction");
        }
        if wire.work_axis_construction.is_some() {
            present.push("work_axis_construction");
        }
        if wire.work_point_construction.is_some() {
            present.push("work_point_construction");
        }
        if wire.hole_construction.is_some() {
            present.push("hole_construction");
        }
        if present.len() > 1 {
            return Err(DesignParameterScopePayloadError(format!(
                "design parameter scope carries more than one payload family: {}",
                present.join(", ")
            )));
        }
        let payload = match &wire.kind {
            DesignFeatureKind::Sketch => DesignScopePayload::Sketch(wire.sketch_entity.take()),
            DesignFeatureKind::Esquisse => DesignScopePayload::Esquisse(wire.sketch_entity.take()),
            DesignFeatureKind::Skizze => DesignScopePayload::Skizze(wire.sketch_entity.take()),
            DesignFeatureKind::Esboco => DesignScopePayload::Esboco(wire.sketch_entity.take()),
            DesignFeatureKind::Assemble => {
                DesignScopePayload::Assemble(wire.assembly_alignment.take())
            }
            DesignFeatureKind::AsBuilt => {
                DesignScopePayload::AsBuilt(wire.assembly_alignment.take())
            }
            DesignFeatureKind::Extrude => DesignScopePayload::Extrude(wire.extrude.take()),
            DesignFeatureKind::Extrusion => DesignScopePayload::Extrusion(wire.extrude.take()),
            DesignFeatureKind::Extrusao => DesignScopePayload::Extrusao(wire.extrude.take()),
            DesignFeatureKind::Fillet => {
                DesignScopePayload::Fillet(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Conge => {
                DesignScopePayload::Conge(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Abrundung => {
                DesignScopePayload::Abrundung(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Arredondamento => {
                DesignScopePayload::Arredondamento(wire.fixed_fillet_parameters.take())
            }
            DesignFeatureKind::Chamfer => {
                DesignScopePayload::Chamfer(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Chanfrein => {
                DesignScopePayload::Chanfrein(wire.fixed_chamfer_parameters.take())
            }
            DesignFeatureKind::Combine => {
                DesignScopePayload::Combine(wire.combine_operation.take())
            }
            DesignFeatureKind::Draft => DesignScopePayload::Draft(wire.draft_operation.take()),
            DesignFeatureKind::ReplaceFace => DesignScopePayload::ReplaceFace,
            DesignFeatureKind::CPattern => {
                DesignScopePayload::CPattern(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::CircularPattern => {
                DesignScopePayload::CircularPattern(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::ReseauC => {
                DesignScopePayload::ReseauC(wire.circular_pattern_construction.take())
            }
            DesignFeatureKind::RPattern => {
                DesignScopePayload::RPattern(wire.rectangular_pattern_construction.take())
            }
            DesignFeatureKind::RectangularPattern => {
                DesignScopePayload::RectangularPattern(wire.rectangular_pattern_construction.take())
            }
            DesignFeatureKind::Mirror => {
                DesignScopePayload::Mirror(wire.mirror_construction.take())
            }
            DesignFeatureKind::SymetrieMiroir => {
                DesignScopePayload::SymetrieMiroir(wire.mirror_construction.take())
            }
            DesignFeatureKind::Move => DesignScopePayload::Move(wire.move_operation.take()),
            DesignFeatureKind::OffsetFaces => {
                DesignScopePayload::OffsetFaces(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::OffsetFaces(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match OffsetFaces".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::DecalerLesFaces => {
                DesignScopePayload::DecalerLesFaces(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::OffsetFaces(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match DecalerLesFaces".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Revolve => {
                DesignScopePayload::Revolve(match wire.path_feature.take() {
                    None => None,
                    Some(DesignPathFeatureWire {
                        path_feature_construction:
                            Some(DesignPathFeatureConstruction::Revolve(value)),
                        sweep_profile: None,
                    }) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "path_feature_construction or sweep_profile does not match Revolve"
                                .into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Shell => {
                DesignScopePayload::Shell(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Shell".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Schale => {
                DesignScopePayload::Schale(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Shell(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Schale".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Thicken => {
                DesignScopePayload::Thicken(match wire.direct_face_operation.take() {
                    None => None,
                    Some(DesignDirectFaceOperation::Thicken(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "direct_face_operation.operation does not match Thicken".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::SpirePrimitive => {
                DesignScopePayload::SpirePrimitive(wire.coil.take())
            }
            DesignFeatureKind::CoilPrimitive => DesignScopePayload::CoilPrimitive(wire.coil.take()),
            DesignFeatureKind::Loft => DesignScopePayload::Loft(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)),
                    sweep_profile: None,
                }) => Some(value),
                Some(_) => {
                    return Err(DesignParameterScopePayloadError(
                        "path_feature_construction or sweep_profile does not match Loft".into(),
                    ))
                }
            }),
            DesignFeatureKind::Sweep => DesignScopePayload::Sweep(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire {
                    path_feature_construction,
                    sweep_profile,
                }) => {
                    let construction = match path_feature_construction {
                        None => None,
                        Some(DesignPathFeatureConstruction::Sweep(value)) => Some(value),
                        Some(_) => {
                            return Err(DesignParameterScopePayloadError(
                                "path_feature_construction.kind does not match Sweep".into(),
                            ))
                        }
                    };
                    Some(DesignSweepScope {
                        construction,
                        sweep_profile,
                    })
                }
            }),
            DesignFeatureKind::Pipe => DesignScopePayload::Pipe(match wire.path_feature.take() {
                None => None,
                Some(DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Pipe(value)),
                    sweep_profile: None,
                }) => Some(value),
                Some(_) => {
                    return Err(DesignParameterScopePayloadError(
                        "path_feature_construction or sweep_profile does not match Pipe".into(),
                    ))
                }
            }),
            DesignFeatureKind::SurfacePatch => {
                DesignScopePayload::SurfacePatch(std::mem::take(&mut wire.surface_patch_boundaries))
            }
            DesignFeatureKind::SurfaceExtend => {
                DesignScopePayload::SurfaceExtend(wire.surface_extend_operation.take())
            }
            DesignFeatureKind::SurfaceOffset => {
                DesignScopePayload::SurfaceOffset(wire.surface_offset_operation.take())
            }
            DesignFeatureKind::SurfaceRuled => DesignScopePayload::SurfaceRuled(
                wire.ruled_surface_operation.take().ok_or_else(|| {
                    DesignParameterScopePayloadError("ruled_surface_operation is required".into())
                })?,
            ),
            DesignFeatureKind::SurfaceTrim => DesignScopePayload::SurfaceTrim,
            DesignFeatureKind::BoundaryFill => DesignScopePayload::BoundaryFill,
            DesignFeatureKind::Hole => DesignScopePayload::Hole(wire.hole_construction.take()),
            DesignFeatureKind::Split => DesignScopePayload::Split,
            DesignFeatureKind::Scale => DesignScopePayload::Scale(wire.scale_operation.take()),
            DesignFeatureKind::Massstab => {
                DesignScopePayload::Massstab(wire.scale_operation.take())
            }
            DesignFeatureKind::Thread => {
                DesignScopePayload::Thread(wire.thread_construction.take())
            }
            DesignFeatureKind::EdgeFlange => {
                DesignScopePayload::EdgeFlange(wire.edge_flange_operation.take())
            }
            DesignFeatureKind::Hem => DesignScopePayload::Hem(wire.hem_operation.take()),
            DesignFeatureKind::BaseFlange => {
                DesignScopePayload::BaseFlange(wire.base_flange.take())
            }
            DesignFeatureKind::ComponentInsert => {
                DesignScopePayload::ComponentInsert(wire.component_insert_construction.take())
            }
            DesignFeatureKind::CopyPaste => {
                DesignScopePayload::CopyPaste(wire.copy_paste_component_operation.take())
            }
            DesignFeatureKind::JointOrigin => {
                DesignScopePayload::JointOrigin(wire.joint_origin_frame.take())
            }
            DesignFeatureKind::Canvas => DesignScopePayload::Canvas,
            DesignFeatureKind::Decal => DesignScopePayload::Decal,
            DesignFeatureKind::BaseMeshFeature => DesignScopePayload::BaseMeshFeature,
            DesignFeatureKind::WorkPlane => {
                DesignScopePayload::WorkPlane(wire.work_plane_frame.take())
            }
            DesignFeatureKind::WorkAxis => {
                DesignScopePayload::WorkAxis(wire.work_axis_construction.take())
            }
            DesignFeatureKind::WorkPoint => {
                DesignScopePayload::WorkPoint(wire.work_point_construction.take())
            }
            DesignFeatureKind::DerivedInstance => {
                DesignScopePayload::DerivedInstance(wire.derived_instance_construction.take())
            }
            DesignFeatureKind::CustomFeature => DesignScopePayload::CustomFeature,
            DesignFeatureKind::Form => DesignScopePayload::Form,
            DesignFeatureKind::SurfaceStitch => DesignScopePayload::SurfaceStitch(
                wire.surface_stitch_operation.take().ok_or_else(|| {
                    DesignParameterScopePayloadError("surface_stitch_operation is required".into())
                })?,
            ),
            DesignFeatureKind::BaseFeature => {
                DesignScopePayload::BaseFeature(wire.base_feature_construction.take())
            }
            DesignFeatureKind::CopyPasteBodies => {
                DesignScopePayload::CopyPasteBodies(wire.copy_paste_bodies_operation.take())
            }
            DesignFeatureKind::SplitFace => DesignScopePayload::SplitFace,
            DesignFeatureKind::DeleteFace => DesignScopePayload::DeleteFace,
            DesignFeatureKind::SurfaceDeleteFace => DesignScopePayload::SurfaceDeleteFace,
            DesignFeatureKind::RemoveBody => DesignScopePayload::RemoveBody,
            DesignFeatureKind::Face => DesignScopePayload::Face,
            DesignFeatureKind::SpherePrimitive => {
                DesignScopePayload::SpherePrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Sphere(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match SpherePrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::TorusPrimitive => {
                DesignScopePayload::TorusPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Torus(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match TorusPrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::BoxPrimitive => {
                DesignScopePayload::BoxPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Box(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match BoxPrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::CylinderPrimitive => {
                DesignScopePayload::CylinderPrimitive(match wire.solid_primitive.take() {
                    None => None,
                    Some(DesignSolidPrimitive::Cylinder(value)) => Some(value),
                    Some(_) => {
                        return Err(DesignParameterScopePayloadError(
                            "solid_primitive.primitive does not match CylinderPrimitive".into(),
                        ))
                    }
                })
            }
            DesignFeatureKind::Native(name) => DesignScopePayload::Native(name.clone()),
        };
        if wire.extrude.is_some()
            || wire.coil.is_some()
            || wire.base_flange.is_some()
            || wire.path_feature.is_some()
            || wire.work_plane_frame.is_some()
            || wire.joint_origin_frame.is_some()
            || wire.sketch_entity.is_some()
            || wire.solid_primitive.is_some()
            || wire.direct_face_operation.is_some()
            || wire.move_operation.is_some()
            || wire.scale_operation.is_some()
            || wire.surface_stitch_operation.is_some()
            || wire.surface_extend_operation.is_some()
            || wire.surface_offset_operation.is_some()
            || wire.ruled_surface_operation.is_some()
            || !wire.surface_patch_boundaries.is_empty()
            || wire.edge_flange_operation.is_some()
            || wire.hem_operation.is_some()
            || wire.fixed_fillet_parameters.is_some()
            || wire.fixed_chamfer_parameters.is_some()
            || wire.combine_operation.is_some()
            || wire.thread_construction.is_some()
            || wire.draft_operation.is_some()
            || wire.circular_pattern_construction.is_some()
            || wire.rectangular_pattern_construction.is_some()
            || wire.assembly_alignment.is_some()
            || wire.component_insert_construction.is_some()
            || wire.derived_instance_construction.is_some()
            || wire.copy_paste_component_operation.is_some()
            || wire.copy_paste_bodies_operation.is_some()
            || wire.mirror_construction.is_some()
            || wire.base_feature_construction.is_some()
            || wire.work_axis_construction.is_some()
            || wire.work_point_construction.is_some()
            || wire.hole_construction.is_some()
        {
            return Err(DesignParameterScopePayloadError(format!(
                "design parameter scope payload disagrees with kind {}",
                wire.kind
            )));
        }
        if wire.history_state_id_offset != wire.kind_offset.saturating_sub(8) {
            return Err(DesignParameterScopePayloadError(
                "history_state_id_offset disagrees with kind_offset".into(),
            ));
        }
        Self::try_new(DesignParameterScopeDraft {
            id: wire.id,
            byte_offset: wire.byte_offset,
            class_tag: wire
                .class_tag
                .try_into()
                .map_err(DesignParameterScopePayloadError)?,
            record_index: wire.record_index,
            frame_length: wire.frame_length,
            kind_offset: wire.kind_offset,
            feature_ordinal: std::num::NonZeroU32::new(wire.feature_ordinal).ok_or_else(|| {
                DesignParameterScopePayloadError("feature_ordinal must be nonzero".into())
            })?,
            feature_ordinal_offset: wire.feature_ordinal_offset,
            history_state_id: wire.history_state_id,
            previous_history_state_id: wire.previous_history_state_id,
            previous_history_state_id_offset: wire.previous_history_state_id_offset,
            reference_count_offset: wire.reference_count_offset,
            reference_members: ReferenceRun::from_columns(
                wire.reference_members,
                wire.reference_member_offsets,
                "reference_members/reference_member_offsets",
            )
            .map_err(DesignParameterScopePayloadError)?,
            payload,
            unclosed_construction_operand_groups: wire.unclosed_construction_operand_groups,
            paired_class_tag: wire
                .paired_class_tag
                .try_into()
                .map_err(DesignParameterScopePayloadError)?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignParameterScope> for DesignParameterScopeSerde {
    fn from(scope: DesignParameterScope) -> Self {
        let kind = scope.kind();
        let history_state_id_offset = scope.history_state_id_offset();
        let (reference_members, reference_member_offsets) = scope.reference_members.into_wire();
        let mut wire = DesignParameterScopeSerde {
            id: scope.id,
            byte_offset: scope.byte_offset,
            class_tag: scope.class_tag.into(),
            record_index: scope.record_index,
            frame_length: scope.frame_length,
            kind,
            kind_offset: scope.kind_offset,
            extrude: None,
            coil: None,
            feature_ordinal: scope.feature_ordinal.get(),
            feature_ordinal_offset: scope.feature_ordinal_offset,
            history_state_id: scope.history_state_id,
            history_state_id_offset,
            previous_history_state_id: scope.previous_history_state_id,
            previous_history_state_id_offset: scope.previous_history_state_id_offset,
            reference_count_offset: scope.reference_count_offset,
            reference_members,
            reference_member_offsets,
            solid_primitive: None,
            direct_face_operation: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            surface_extend_operation: None,
            surface_offset_operation: None,
            ruled_surface_operation: None,
            base_flange: None,
            surface_patch_boundaries: Vec::new(),
            edge_flange_operation: None,
            hem_operation: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature: None,
            combine_operation: None,
            thread_construction: None,
            draft_operation: None,
            circular_pattern_construction: None,
            rectangular_pattern_construction: None,
            assembly_alignment: None,
            component_insert_construction: None,
            derived_instance_construction: None,
            copy_paste_component_operation: None,
            copy_paste_bodies_operation: None,
            mirror_construction: None,
            base_feature_construction: None,
            work_plane_frame: None,
            work_axis_construction: None,
            joint_origin_frame: None,
            work_point_construction: None,
            unclosed_construction_operand_groups: scope.unclosed_construction_operand_groups,
            hole_construction: None,
            sketch_entity: None,
            paired_class_tag: scope.paired_class_tag.into(),
            paired_byte_offset: scope.paired_byte_offset,
        };
        match scope.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value) => wire.extrude = value,
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => wire.coil = value,
            DesignScopePayload::BaseFlange(value) => wire.base_flange = value,
            DesignScopePayload::Revolve(value) => {
                wire.path_feature = value.map(|value| DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Revolve(value)),
                    sweep_profile: None,
                });
            }
            DesignScopePayload::Loft(value) => {
                wire.path_feature = value.map(|value| DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Loft(value)),
                    sweep_profile: None,
                });
            }
            DesignScopePayload::Sweep(value) => {
                wire.path_feature = value.map(|sweep| DesignPathFeatureWire {
                    path_feature_construction: sweep
                        .construction
                        .map(DesignPathFeatureConstruction::Sweep),
                    sweep_profile: sweep.sweep_profile,
                });
            }
            DesignScopePayload::Pipe(value) => {
                wire.path_feature = value.map(|value| DesignPathFeatureWire {
                    path_feature_construction: Some(DesignPathFeatureConstruction::Pipe(value)),
                    sweep_profile: None,
                });
            }
            DesignScopePayload::WorkPlane(value) => wire.work_plane_frame = value,
            DesignScopePayload::JointOrigin(value) => wire.joint_origin_frame = value,
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value) => wire.sketch_entity = value,
            DesignScopePayload::SpherePrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Sphere);
            }
            DesignScopePayload::TorusPrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Torus);
            }
            DesignScopePayload::BoxPrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Box);
            }
            DesignScopePayload::CylinderPrimitive(value) => {
                wire.solid_primitive = value.map(DesignSolidPrimitive::Cylinder);
            }
            DesignScopePayload::ReplaceFace => {}
            DesignScopePayload::OffsetFaces(value) | DesignScopePayload::DecalerLesFaces(value) => {
                wire.direct_face_operation = value.map(DesignDirectFaceOperation::OffsetFaces);
            }
            DesignScopePayload::Shell(value) | DesignScopePayload::Schale(value) => {
                wire.direct_face_operation = value.map(DesignDirectFaceOperation::Shell);
            }
            DesignScopePayload::Thicken(value) => {
                wire.direct_face_operation = value.map(DesignDirectFaceOperation::Thicken);
            }
            DesignScopePayload::Move(value) => wire.move_operation = value,
            DesignScopePayload::Scale(value) | DesignScopePayload::Massstab(value) => {
                wire.scale_operation = value;
            }
            DesignScopePayload::SurfaceStitch(value) => wire.surface_stitch_operation = Some(value),
            DesignScopePayload::SurfaceExtend(value) => wire.surface_extend_operation = value,
            DesignScopePayload::SurfaceOffset(value) => wire.surface_offset_operation = value,
            DesignScopePayload::SurfaceRuled(value) => wire.ruled_surface_operation = Some(value),
            DesignScopePayload::SurfacePatch(value) => wire.surface_patch_boundaries = value,
            DesignScopePayload::EdgeFlange(value) => wire.edge_flange_operation = value,
            DesignScopePayload::Hem(value) => wire.hem_operation = value,
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value) => wire.fixed_fillet_parameters = value,
            DesignScopePayload::Chamfer(value) | DesignScopePayload::Chanfrein(value) => {
                wire.fixed_chamfer_parameters = value;
            }
            DesignScopePayload::Combine(value) => wire.combine_operation = value,
            DesignScopePayload::Thread(value) => wire.thread_construction = value,
            DesignScopePayload::Draft(value) => wire.draft_operation = value,
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => wire.circular_pattern_construction = value,
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                wire.rectangular_pattern_construction = value;
            }
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                wire.assembly_alignment = value;
            }
            DesignScopePayload::ComponentInsert(value) => {
                wire.component_insert_construction = value;
            }
            DesignScopePayload::DerivedInstance(value) => {
                wire.derived_instance_construction = value;
            }
            DesignScopePayload::CopyPaste(value) => wire.copy_paste_component_operation = value,
            DesignScopePayload::CopyPasteBodies(value) => wire.copy_paste_bodies_operation = value,
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                wire.mirror_construction = value;
            }
            DesignScopePayload::BaseFeature(value) => wire.base_feature_construction = value,
            DesignScopePayload::WorkAxis(value) => wire.work_axis_construction = value,
            DesignScopePayload::WorkPoint(value) => wire.work_point_construction = value,
            DesignScopePayload::Hole(value) => wire.hole_construction = value,
            DesignScopePayload::SurfaceTrim
            | DesignScopePayload::BoundaryFill
            | DesignScopePayload::Split
            | DesignScopePayload::Canvas
            | DesignScopePayload::Decal
            | DesignScopePayload::BaseMeshFeature
            | DesignScopePayload::CustomFeature
            | DesignScopePayload::Form
            | DesignScopePayload::SplitFace
            | DesignScopePayload::DeleteFace
            | DesignScopePayload::SurfaceDeleteFace
            | DesignScopePayload::RemoveBody
            | DesignScopePayload::Face
            | DesignScopePayload::Native(_) => {}
        }
        wire
    }
}

impl DesignParameterScope {
    pub(crate) fn try_new(
        draft: DesignParameterScopeDraft,
    ) -> Result<Self, DesignParameterScopePayloadError> {
        let fail = |field: &str| {
            DesignParameterScopePayloadError(format!("invalid parameter scope {field}"))
        };
        let kind = draft.payload.kind_name();
        if draft.frame_length <= 89
            || draft.byte_offset.checked_add(draft.frame_length) != Some(draft.paired_byte_offset)
        {
            return Err(fail("frame_length/paired_byte_offset"));
        }
        if !(draft.byte_offset < draft.kind_offset
            && draft.kind_offset < draft.feature_ordinal_offset)
        {
            return Err(fail("kind_offset/feature_ordinal_offset"));
        }
        let tail = draft
            .paired_byte_offset
            .checked_sub(draft.feature_ordinal_offset)
            .and_then(|length| usize::try_from(length).ok())
            .ok_or_else(|| fail("feature_ordinal_offset"))?;
        if !crate::design::decode::scopes::parameter_scope_tail_length_is_valid(kind, tail) {
            return Err(fail("feature_ordinal_offset/kind"));
        }
        match draft.previous_history_state_id_offset {
            None if draft.previous_history_state_id.is_none() => {}
            Some(offset) => {
                let relative =
                    crate::design::decode::scopes::parameter_scope_previous_history_offset(
                        kind, tail,
                    )
                    .ok_or_else(|| fail("previous_history_state_id_offset"))?;
                if draft.feature_ordinal_offset.checked_add(relative as u64) != Some(offset)
                    || draft.history_state_id.is_some() != draft.previous_history_state_id.is_some()
                {
                    return Err(fail("previous_history_state_id_offset/history_state_id/previous_history_state_id"));
                }
            }
            None => {
                return Err(fail(
                    "previous_history_state_id_offset/previous_history_state_id",
                ))
            }
        }
        if !(draft.byte_offset < draft.reference_count_offset
            && draft.reference_count_offset < draft.kind_offset)
            || draft.reference_members.is_empty()
        {
            return Err(fail("reference_count_offset/reference_members"));
        }
        let mut expected = draft
            .reference_count_offset
            .checked_add(5)
            .ok_or_else(|| fail("reference_count_offset"))?;
        for member in draft.reference_members.offsets() {
            if *member != expected
                || *member <= draft.reference_count_offset
                || *member >= draft.kind_offset
            {
                return Err(fail("reference_member_offsets"));
            }
            expected = expected
                .checked_add(11)
                .ok_or_else(|| fail("reference_member_offsets"))?;
        }
        if draft.reference_members.offsets().count() != draft.reference_members.len()
            || draft
                .reference_members
                .offsets()
                .next_back()
                .and_then(|offset| offset.checked_add(18))
                != Some(draft.kind_offset)
        {
            return Err(fail("reference_member_offsets/kind_offset"));
        }
        Ok(Self {
            id: draft.id,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            record_index: draft.record_index,
            frame_length: draft.frame_length,
            kind_offset: draft.kind_offset,
            feature_ordinal: draft.feature_ordinal,
            feature_ordinal_offset: draft.feature_ordinal_offset,
            history_state_id: draft.history_state_id,
            previous_history_state_id: draft.previous_history_state_id,
            previous_history_state_id_offset: draft.previous_history_state_id_offset,
            reference_count_offset: draft.reference_count_offset,
            reference_members: draft.reference_members,
            payload: draft.payload,
            unclosed_construction_operand_groups: draft.unclosed_construction_operand_groups,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: draft.paired_byte_offset,
        })
    }

    pub(crate) fn into_draft(self) -> DesignParameterScopeDraft {
        DesignParameterScopeDraft {
            id: self.id,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag,
            record_index: self.record_index,
            frame_length: self.frame_length,
            kind_offset: self.kind_offset,
            feature_ordinal: self.feature_ordinal,
            feature_ordinal_offset: self.feature_ordinal_offset,
            history_state_id: self.history_state_id,
            previous_history_state_id: self.previous_history_state_id,
            previous_history_state_id_offset: self.previous_history_state_id_offset,
            reference_count_offset: self.reference_count_offset,
            reference_members: self.reference_members,
            payload: self.payload,
            unclosed_construction_operand_groups: self.unclosed_construction_operand_groups,
            paired_class_tag: self.paired_class_tag,
            paired_byte_offset: self.paired_byte_offset,
        }
    }

    pub(crate) fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut DesignParameterScopeDraft),
    ) -> Result<(), DesignParameterScopePayloadError> {
        let mut draft = self.clone().into_draft();
        edit(&mut draft);
        *self = Self::try_new(draft)?;
        Ok(())
    }

    pub(crate) fn payload_mut(&mut self) -> DesignScopePayloadMut<'_> {
        self.payload.fields_mut()
    }

    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn frame_length(&self) -> u64 {
        self.frame_length
    }
    pub(crate) fn kind_offset(&self) -> u64 {
        self.kind_offset
    }
    pub(crate) fn feature_ordinal_offset(&self) -> u64 {
        self.feature_ordinal_offset
    }
    pub(crate) fn history_state_id(&self) -> Option<i64> {
        self.history_state_id
    }
    pub(crate) fn previous_history_state_id(&self) -> Option<i64> {
        self.previous_history_state_id
    }
    pub(crate) fn previous_history_state_id_offset(&self) -> Option<u64> {
        self.previous_history_state_id_offset
    }
    pub(crate) fn reference_count_offset(&self) -> u64 {
        self.reference_count_offset
    }
    pub(crate) fn reference_members(&self) -> &ReferenceRun<u32> {
        &self.reference_members
    }
    pub(crate) fn payload(&self) -> &DesignScopePayload {
        &self.payload
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.paired_byte_offset
    }
}

impl DesignParameterScope {
    /// Byte offset of the state word before the length-prefixed kind name.
    pub fn history_state_id_offset(&self) -> u64 {
        self.kind_offset.saturating_sub(8)
    }

    /// Source feature-family name, derived from its construction variant.
    pub(crate) fn kind(&self) -> DesignFeatureKind {
        self.payload.kind()
    }

    /// Source spelling without allocating a kind tag.
    pub(crate) fn kind_name(&self) -> &str {
        self.payload.kind_name()
    }

    pub(crate) fn extrude(&self) -> Option<&DesignExtrudeScope> {
        match &self.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value) => value.as_ref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn extrude_mut(&mut self) -> Option<&mut DesignExtrudeScope> {
        match &mut self.payload {
            DesignScopePayload::Extrude(value)
            | DesignScopePayload::Extrusion(value)
            | DesignScopePayload::Extrusao(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn coil(&self) -> Option<&DesignCoilScope> {
        match &self.payload {
            DesignScopePayload::SpirePrimitive(value)
            | DesignScopePayload::CoilPrimitive(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn base_flange(&self) -> Option<&DesignBaseFlangeScope> {
        match &self.payload {
            DesignScopePayload::BaseFlange(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_plane_frame(&self) -> Option<&DesignWorkPlaneTransform> {
        match &self.payload {
            DesignScopePayload::WorkPlane(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_plane_frame_mut(&mut self) -> Option<&mut DesignWorkPlaneTransform> {
        match &mut self.payload {
            DesignScopePayload::WorkPlane(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn joint_origin_frame(&self) -> Option<&DesignJointOriginTransform> {
        match &self.payload {
            DesignScopePayload::JointOrigin(value) => value.as_ref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn joint_origin_frame_mut(&mut self) -> Option<&mut DesignJointOriginTransform> {
        match &mut self.payload {
            DesignScopePayload::JointOrigin(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn sketch_entity(&self) -> Option<&DesignSketchEntityBinding> {
        match &self.payload {
            DesignScopePayload::Sketch(value)
            | DesignScopePayload::Esquisse(value)
            | DesignScopePayload::Skizze(value)
            | DesignScopePayload::Esboco(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn move_operation(&self) -> Option<&DesignMoveOperation> {
        match &self.payload {
            DesignScopePayload::Move(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn scale_operation(&self) -> Option<&DesignScaleOperation> {
        match &self.payload {
            DesignScopePayload::Scale(value) | DesignScopePayload::Massstab(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn surface_stitch_operation(&self) -> Option<&DesignSurfaceStitchOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceStitch(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn surface_extend_operation(&self) -> Option<&DesignSurfaceExtendOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceExtend(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn surface_offset_operation(&self) -> Option<&DesignSurfaceOffsetOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceOffset(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn ruled_surface_operation(&self) -> Option<&DesignRuledSurfaceOperation> {
        match &self.payload {
            DesignScopePayload::SurfaceRuled(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn edge_flange_operation(&self) -> Option<&DesignEdgeFlangeOperation> {
        match &self.payload {
            DesignScopePayload::EdgeFlange(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hem_operation(&self) -> Option<&DesignHemOperation> {
        match &self.payload {
            DesignScopePayload::Hem(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn fixed_fillet_parameters(&self) -> Option<&DesignFixedFilletParameters> {
        match &self.payload {
            DesignScopePayload::Fillet(value)
            | DesignScopePayload::Conge(value)
            | DesignScopePayload::Abrundung(value)
            | DesignScopePayload::Arredondamento(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn fixed_chamfer_parameters(&self) -> Option<&DesignFixedChamferParameters> {
        match &self.payload {
            DesignScopePayload::Chamfer(value) | DesignScopePayload::Chanfrein(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn combine_operation(&self) -> Option<&DesignCombineOperation> {
        match &self.payload {
            DesignScopePayload::Combine(value) => value.as_ref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn combine_operation_mut(&mut self) -> Option<&mut DesignCombineOperation> {
        match &mut self.payload {
            DesignScopePayload::Combine(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn thread_construction(&self) -> Option<&DesignThreadConstruction> {
        match &self.payload {
            DesignScopePayload::Thread(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn draft_operation(&self) -> Option<&DesignDraftOperation> {
        match &self.payload {
            DesignScopePayload::Draft(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn circular_pattern_construction(
        &self,
    ) -> Option<&DesignCircularPatternConstruction> {
        match &self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn circular_pattern_construction_mut(
        &mut self,
    ) -> Option<&mut DesignCircularPatternConstruction> {
        match &mut self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn rectangular_pattern_construction(
        &self,
    ) -> Option<&DesignRectangularPatternConstruction> {
        match &self.payload {
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn rectangular_pattern_construction_mut(
        &mut self,
    ) -> Option<&mut DesignRectangularPatternConstruction> {
        match &mut self.payload {
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn assembly_alignment(&self) -> Option<&DesignAssemblyAlignment> {
        match &self.payload {
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn assembly_alignment_mut(&mut self) -> Option<&mut DesignAssemblyAlignment> {
        match &mut self.payload {
            DesignScopePayload::Assemble(value) | DesignScopePayload::AsBuilt(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn component_insert_construction(
        &self,
    ) -> Option<&DesignComponentInsertConstruction> {
        match &self.payload {
            DesignScopePayload::ComponentInsert(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn derived_instance_construction(
        &self,
    ) -> Option<&DesignDerivedInstanceConstruction> {
        match &self.payload {
            DesignScopePayload::DerivedInstance(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_component_operation(
        &self,
    ) -> Option<&DesignCopyPasteComponentOperation> {
        match &self.payload {
            DesignScopePayload::CopyPaste(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn copy_paste_bodies_operation(&self) -> Option<&DesignCopyPasteBodiesOperation> {
        match &self.payload {
            DesignScopePayload::CopyPasteBodies(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn mirror_construction(&self) -> Option<&DesignMirrorConstruction> {
        match &self.payload {
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                value.as_ref()
            }
            _ => None,
        }
    }

    pub(crate) fn mirror_construction_mut(&mut self) -> Option<&mut DesignMirrorConstruction> {
        match &mut self.payload {
            DesignScopePayload::Mirror(value) | DesignScopePayload::SymetrieMiroir(value) => {
                value.as_mut()
            }
            _ => None,
        }
    }

    pub(crate) fn base_feature_construction(&self) -> Option<&DesignBaseFeatureConstruction> {
        match &self.payload {
            DesignScopePayload::BaseFeature(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_axis_construction(&self) -> Option<&DesignWorkAxisConstruction> {
        match &self.payload {
            DesignScopePayload::WorkAxis(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_point_construction(&self) -> Option<&DesignWorkPointConstruction> {
        match &self.payload {
            DesignScopePayload::WorkPoint(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn work_point_construction_mut(
        &mut self,
    ) -> Option<&mut DesignWorkPointConstruction> {
        match &mut self.payload {
            DesignScopePayload::WorkPoint(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn hole_construction(&self) -> Option<&DesignHoleConstruction> {
        match &self.payload {
            DesignScopePayload::Hole(value) => value.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn hole_construction_mut(&mut self) -> Option<&mut DesignHoleConstruction> {
        match &mut self.payload {
            DesignScopePayload::Hole(value) => value.as_mut(),
            _ => None,
        }
    }

    pub(crate) fn surface_patch_boundaries(&self) -> &[DesignSurfacePatchBoundary] {
        match &self.payload {
            DesignScopePayload::SurfacePatch(value) => value,
            _ => &[],
        }
    }

    pub(crate) fn extrude_prologue(&self) -> Option<DesignExtrudePrologue> {
        self.extrude().and_then(|extrude| extrude.extrude_prologue)
    }

    #[cfg(test)]
    pub(crate) fn extrude_prologue_mut(&mut self) -> Option<&mut DesignExtrudePrologue> {
        self.extrude_mut()
            .and_then(|extrude| extrude.extrude_prologue.as_mut())
    }

    pub(crate) fn extrude_profile(&self) -> Option<&DesignSketchProfileOperand> {
        self.extrude()
            .and_then(|extrude| extrude.extrude_profile.as_ref())
    }

    #[cfg(test)]
    pub(crate) fn extrude_profile_mut(&mut self) -> Option<&mut DesignSketchProfileOperand> {
        self.extrude_mut()
            .and_then(|extrude| extrude.extrude_profile.as_mut())
    }

    pub(crate) fn fixed_extrude_parameters(&self) -> Option<&DesignFixedExtrudeParameters> {
        self.extrude()
            .and_then(|extrude| extrude.fixed_extrude_parameters.as_ref())
    }

    pub(crate) fn base_flange_operation(&self) -> Option<&DesignBaseFlangeOperation> {
        self.base_flange()
            .and_then(|base_flange| base_flange.base_flange_operation.as_ref())
    }

    pub(crate) fn base_flange_profile(&self) -> Option<&DesignSketchProfileOperand> {
        self.base_flange()
            .and_then(|base_flange| base_flange.base_flange_profile.as_ref())
    }

    pub(crate) fn coil_operation(&self) -> Option<DesignExtrudeOperation> {
        self.coil()
            .and_then(|coil| coil.coil_operation.map(|field| field.value))
    }

    pub(crate) fn coil_operation_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.coil_operation.map(|field| field.offset))
    }

    pub(crate) fn coil_extent(&self) -> Option<DesignCoilExtent> {
        self.coil()
            .and_then(|coil| coil.coil_extent.map(|field| field.value()))
    }

    pub(crate) fn coil_section(&self) -> Option<DesignCoilSection> {
        self.coil()
            .and_then(|coil| coil.coil_section.map(|field| field.value()))
    }

    pub(crate) fn coil_section_placement(&self) -> Option<DesignCoilSectionPlacement> {
        self.coil()
            .and_then(|coil| coil.coil_section_placement.map(|field| field.value()))
    }

    pub(crate) fn coil_clockwise(&self) -> Option<bool> {
        self.coil()
            .and_then(|coil| coil.coil_clockwise.map(|field| field.value()))
    }

    #[cfg(test)]
    pub(crate) fn coil_extent_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.coil_extent.and_then(|field| field.offset()))
    }

    #[cfg(test)]
    pub(crate) fn coil_section_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.coil_section.and_then(|field| field.offset()))
    }

    #[cfg(test)]
    pub(crate) fn coil_section_placement_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.coil_section_placement.and_then(|field| field.offset()))
    }

    #[cfg(test)]
    pub(crate) fn coil_clockwise_offset(&self) -> Option<u64> {
        self.coil()
            .and_then(|coil| coil.coil_clockwise.and_then(|field| field.offset()))
    }

    pub(crate) fn coil_placement(&self) -> Option<&DesignCoilPlacement> {
        self.coil().and_then(|coil| coil.coil_placement.as_ref())
    }

    pub(crate) fn coil_transform(&self) -> Option<&DesignCoilTransform> {
        self.coil().and_then(|coil| coil.coil_transform.as_ref())
    }

    pub(crate) fn has_path_construction(&self) -> bool {
        match &self.payload {
            DesignScopePayload::Revolve(value) => value.is_some(),
            DesignScopePayload::Loft(value) => value.is_some(),
            DesignScopePayload::Pipe(value) => value.is_some(),
            DesignScopePayload::Sweep(value) => value
                .as_ref()
                .is_some_and(|sweep| sweep.construction.is_some()),
            _ => false,
        }
    }

    pub(crate) fn sweep_profile(&self) -> Option<&DesignSketchProfileOperand> {
        match &self.payload {
            DesignScopePayload::Sweep(value) => value
                .as_ref()
                .and_then(|sweep| sweep.sweep_profile.as_ref()),
            _ => None,
        }
    }

    pub(crate) fn work_plane_transform(&self) -> Option<SketchPlacementMatrix> {
        self.work_plane_frame()
            .map(|frame| frame.work_plane_transform)
    }

    pub(crate) fn work_plane_reference(&self) -> Option<u32> {
        self.work_plane_frame()
            .and_then(|frame| frame.reference.as_ref().map(|r| r.work_plane_reference))
    }

    pub(crate) fn work_plane_construction(&self) -> Option<&DesignWorkPlaneConstruction> {
        self.work_plane_frame()
            .and_then(|frame| frame.work_plane_construction.as_ref())
    }

    pub(crate) fn work_plane_construction_mut(
        &mut self,
    ) -> Option<&mut DesignWorkPlaneConstruction> {
        self.work_plane_frame_mut()
            .and_then(|frame| frame.work_plane_construction.as_mut())
    }

    pub(crate) fn joint_origin_transform(&self) -> Option<SketchPlacementMatrix> {
        self.joint_origin_frame()
            .map(|frame| frame.joint_origin_transform)
    }

    pub(crate) fn joint_origin_transform_offset(&self) -> Option<u64> {
        self.joint_origin_frame()
            .map(|frame| frame.joint_origin_transform_offset)
    }

    #[cfg(test)]
    pub(crate) fn joint_origin_reference(&self) -> Option<u32> {
        self.joint_origin_frame().and_then(|frame| {
            frame
                .reference
                .as_ref()
                .map(|reference| reference.joint_origin_reference)
        })
    }

    #[cfg(test)]
    pub(crate) fn joint_origin_reference_offset(&self) -> Option<u64> {
        self.joint_origin_frame().and_then(|frame| {
            frame
                .reference
                .as_ref()
                .map(|reference| reference.joint_origin_reference_offset)
        })
    }
}

#[cfg(test)]
impl DesignParameterScope {
    /// Build a scope carrying only its identity, kind, and record index.
    pub(crate) fn with_work_plane_transform(&mut self, transform: SketchPlacementMatrix) {
        self.payload = DesignScopePayload::WorkPlane(Some(DesignWorkPlaneTransform {
            work_plane_transform: transform,
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: None,
        }));
    }

    pub(crate) fn with_work_plane_reference(&mut self, record_index: u32) {
        if let Some(frame) = self.work_plane_frame_mut() {
            frame.reference = Some(DesignWorkPlaneReference {
                work_plane_reference: record_index,
                work_plane_reference_offset: 0,
            });
        }
    }

    pub(crate) fn with_joint_origin_transform(&mut self, transform: SketchPlacementMatrix) {
        self.payload = DesignScopePayload::JointOrigin(Some(DesignJointOriginTransform {
            joint_origin_transform: transform,
            joint_origin_transform_offset: 0,
            reference: None,
        }));
    }

    pub(crate) fn empty<P>(id: &str, payload: P, record_index: u32) -> Self
    where
        P: TryInto<DesignScopePayload>,
        P::Error: std::fmt::Debug,
    {
        Self::try_new(DesignParameterScopeDraft {
            id: id.to_string(),
            byte_offset: 0,
            class_tag: DesignClassTag::try_from("256".to_owned()).unwrap(),
            record_index,
            frame_length: 128,
            kind_offset: 32,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 48,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: ReferenceRun::located(vec![Located {
                value: record_index,
                offset: 14,
            }]),
            payload: payload.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: DesignClassTag::try_from("257".to_owned()).unwrap(),
            paired_byte_offset: 128,
        })
        .unwrap()
    }
}

/// Height extent law carried by a sheet-metal `EdgeFlange` scope.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DesignEdgeFlangeHeightExtent {
    /// The flange height is a direct distance from the selected sheet datum.
    #[default]
    Distance,
    /// The flange height is measured from a selected construction entity.
    ToObject {
        /// Role-`0x21` construction-operand group containing the target.
        target_group_record_index: u32,
        /// Entity-selection operand carried by the target group.
        target_operand_record_index: u32,
        /// Parameter owner carrying the signed target offset.
        offset_owner_record_index: u32,
        /// Two marked references inserted in the fixed operation section.
        reference_record_indices: [u32; 2],
    },
}

/// A group record index with a representable recipe index three records later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct DesignRecipeGroupIndex(u32);

impl DesignRecipeGroupIndex {
    pub(crate) fn get(self) -> u32 {
        self.0
    }
    pub(crate) fn operand(self) -> u32 {
        self.0 + 3
    }
}

impl TryFrom<u32> for DesignRecipeGroupIndex {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        value
            .checked_add(3)
            .ok_or("group_record_index + 3 overflows")?;
        Ok(Self(value))
    }
}

impl From<DesignRecipeGroupIndex> for u32 {
    fn from(value: DesignRecipeGroupIndex) -> Self {
        value.get()
    }
}

/// One selected flange edge and its aggregate operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
pub struct DesignEdgeFlangeEdge {
    pub wrapper_record_index: u32,
    pub group_record_index: DesignRecipeGroupIndex,
    pub aggregate_operand_record_index: u32,
}

impl DesignEdgeFlangeEdge {
    pub(crate) fn operand_record_index(&self) -> u32 {
        self.group_record_index.operand()
    }

    pub(crate) fn from_columns(
        wrappers: Vec<u32>,
        groups: Vec<u32>,
        operands: &[u32],
        aggregate_operands: Vec<u32>,
    ) -> Result<Vec<Self>, String> {
        if groups.len() != wrappers.len()
            || operands.len() != wrappers.len()
            || aggregate_operands.len() != wrappers.len()
        {
            return Err("edge_wrapper_record_indices, edge_group_record_indices, edge_operand_record_indices, and aggregate_operand_record_indices must have equal lengths".into());
        }
        if groups
            .iter()
            .zip(operands)
            .any(|(group, operand)| Some(*operand) != group.checked_add(3))
        {
            return Err(
                "edge_operand_record_indices must equal edge_group_record_indices + 3".into(),
            );
        }
        wrappers
            .into_iter()
            .zip(groups)
            .zip(aggregate_operands)
            .map(
                |((wrapper_record_index, group_record_index), aggregate_operand_record_index)| {
                    Ok(Self {
                        wrapper_record_index,
                        group_record_index: group_record_index.try_into()?,
                        aggregate_operand_record_index,
                    })
                },
            )
            .collect()
    }
}

/// A positive finite source scalar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct DesignPositiveScalar(f64);

impl DesignPositiveScalar {
    /// Admit a positive finite scalar.
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    /// The source scalar value.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for DesignPositiveScalar {
    type Error = &'static str;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("scalar must be positive and finite")
    }
}

impl From<DesignPositiveScalar> for f64 {
    fn from(value: DesignPositiveScalar) -> Self {
        value.get()
    }
}

/// A finite source scalar with unrestricted sign.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct DesignFiniteScalar(f64);

impl DesignFiniteScalar {
    /// Admit a finite scalar.
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }
    /// The source scalar value.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for DesignFiniteScalar {
    type Error = &'static str;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("scalar must be finite")
    }
}

impl From<DesignFiniteScalar> for f64 {
    fn from(value: DesignFiniteScalar) -> Self {
        value.get()
    }
}

/// Fixed construction carried by a sheet-metal `EdgeFlange` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignEdgeFlangeOperationSerde",
    into = "DesignEdgeFlangeOperationSerde"
)]
pub struct DesignEdgeFlangeOperation {
    /// Selected flange edges and their aggregate operand group.
    pub selection: DesignEdgeFlangeSelection,
    /// Height parameter-owner record.
    pub height_owner_record_index: u32,
    /// Angle parameter-owner record.
    pub angle_owner_record_index: u32,
    /// Scope references retained by a classed layout after typed roles and
    /// width owners have been claimed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auxiliary_reference_record_indices: Vec<u32>,
    /// Indexed operation-settings record.
    pub settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    pub bend_radius: DesignPositiveScalar,
    /// Byte offset of `bend_radius`.
    pub bend_radius_offset: u64,
    /// Face pair the flange height is measured from.
    pub height_datum: DesignSheetMetalHeightDatum,
    /// Bend position relative to the selected edge.
    pub bend_position: DesignBendPosition,
}

/// Flange edge shape paired with its aggregate operand group.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignEdgeFlangeSelection {
    shape: DesignEdgeFlangeShape,
    aggregate_group_record_index: u32,
}

impl DesignEdgeFlangeSelection {
    pub(crate) fn try_new(
        shape: DesignEdgeFlangeShape,
        aggregate_group_record_index: u32,
    ) -> Result<Self, String> {
        let mismatched_aggregate = {
            let mut edges = shape.edges();
            let first = edges.next();
            edges.next().is_none()
                && first.is_some_and(|edge| {
                    Some(edge.aggregate_operand_record_index)
                        != aggregate_group_record_index.checked_add(3)
                })
        };
        if mismatched_aggregate {
            return Err("single-edge aggregate_operand_record_indices must equal aggregate_group_record_index + 3".into());
        }
        Ok(Self {
            shape,
            aggregate_group_record_index,
        })
    }
    pub(crate) fn shape(&self) -> &DesignEdgeFlangeShape {
        &self.shape
    }
    pub(crate) fn aggregate_group_record_index(&self) -> u32 {
        self.aggregate_group_record_index
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignEdgeFlangeOperationSerde {
    edge_wrapper_record_indices: Vec<u32>,
    edge_group_record_indices: Vec<u32>,
    edge_operand_record_indices: Vec<u32>,
    aggregate_group_record_index: u32,
    aggregate_operand_record_indices: Vec<u32>,
    height_owner_record_index: u32,
    #[serde(default)]
    height_extent: DesignEdgeFlangeHeightExtent,
    angle_owner_record_index: u32,
    #[serde(default)]
    width_mode: Option<DesignEdgeWidthMode>,
    width_distance_owner_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    width_distance_owner_record_indices_by_edge: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_reference_record_indices: Vec<u32>,
    #[serde(default)]
    width_parameter_source: DesignEdgeFlangeWidthParameterSource,
    settings_record_index: u32,
    bend_radius: f64,
    bend_radius_offset: u64,
    reference_side_code: u32,
    height_datum: DesignSheetMetalHeightDatum,
    bend_position: DesignBendPosition,
}

impl TryFrom<DesignEdgeFlangeOperationSerde> for DesignEdgeFlangeOperation {
    type Error = String;

    fn try_from(wire: DesignEdgeFlangeOperationSerde) -> Result<Self, Self::Error> {
        if wire.reference_side_code != 4 {
            return Err("reference_side_code must be 4".into());
        }
        let edges = DesignEdgeFlangeEdge::from_columns(
            wire.edge_wrapper_record_indices,
            wire.edge_group_record_indices,
            &wire.edge_operand_record_indices,
            wire.aggregate_operand_record_indices,
        )?;
        Ok(Self {
            selection: DesignEdgeFlangeSelection::try_new(
                DesignEdgeFlangeShape::from_wire(
                    edges,
                    wire.width_mode,
                    wire.width_distance_owner_record_indices,
                    wire.width_distance_owner_record_indices_by_edge,
                    wire.width_parameter_source,
                    wire.height_extent,
                )?,
                wire.aggregate_group_record_index,
            )?,
            height_owner_record_index: wire.height_owner_record_index,
            angle_owner_record_index: wire.angle_owner_record_index,
            auxiliary_reference_record_indices: wire.auxiliary_reference_record_indices,
            settings_record_index: wire.settings_record_index,
            bend_radius: DesignPositiveScalar::new(wire.bend_radius)
                .ok_or("bend_radius must be positive and finite")?,
            bend_radius_offset: wire.bend_radius_offset,
            height_datum: wire.height_datum,
            bend_position: wire.bend_position,
        })
    }
}

impl From<DesignEdgeFlangeOperation> for DesignEdgeFlangeOperationSerde {
    fn from(operation: DesignEdgeFlangeOperation) -> Self {
        let width_mode = Some(operation.selection.shape().mode());
        let width_distance_owner_record_indices = operation
            .selection
            .shape()
            .owner_indices()
            .copied()
            .collect();
        let width_distance_owner_record_indices_by_edge = match operation.selection.shape() {
            DesignEdgeFlangeShape::TwoSidesPerEdge { edges, .. } => {
                edges.iter().map(|row| row.owners).collect()
            }
            _ => Vec::new(),
        };
        Self {
            edge_wrapper_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.wrapper_record_index)
                .collect(),
            edge_group_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.group_record_index.get())
                .collect(),
            edge_operand_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(DesignEdgeFlangeEdge::operand_record_index)
                .collect(),
            aggregate_group_record_index: operation.selection.aggregate_group_record_index(),
            aggregate_operand_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.aggregate_operand_record_index)
                .collect(),
            height_owner_record_index: operation.height_owner_record_index,
            height_extent: operation.selection.shape().height(),
            angle_owner_record_index: operation.angle_owner_record_index,
            width_mode,
            width_distance_owner_record_indices,
            width_distance_owner_record_indices_by_edge,
            auxiliary_reference_record_indices: operation.auxiliary_reference_record_indices,
            width_parameter_source: operation.selection.shape().source(),
            settings_record_index: operation.settings_record_index,
            bend_radius: operation.bend_radius.get(),
            bend_radius_offset: operation.bend_radius_offset,
            reference_side_code: 4,
            height_datum: operation.height_datum,
            bend_position: operation.bend_position,
        }
    }
}

/// Parameter-owner layout carried by a sheet-metal `Hem` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignHemParameterOwners {
    /// Flat and open forms own a gap and a length.
    GapLength {
        /// Gap parameter-owner record.
        gap_owner_record_index: u32,
        /// Length parameter-owner record.
        length_owner_record_index: u32,
    },
    /// Rolled form owns a radius and an angle.
    RadiusAngle {
        /// Radius parameter-owner record.
        radius_owner_record_index: u32,
        /// Angle parameter-owner record.
        angle_owner_record_index: u32,
    },
    /// Teardrop form owns a gap, a length, and a radius.
    GapLengthRadius {
        /// Gap parameter-owner record.
        gap_owner_record_index: u32,
        /// Length parameter-owner record.
        length_owner_record_index: u32,
        /// Radius parameter-owner record.
        radius_owner_record_index: u32,
    },
}

/// Fixed operation section and parameter-owner layout carried by a sheet-metal
/// `Hem` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignHemOperationWire", into = "DesignHemOperationWire")]
pub struct DesignHemOperation {
    /// Selection-wrapper record for the hem edge.
    pub edge_wrapper_record_index: u32,
    /// Role-`0x08` operand-group record.
    pub edge_group_record_index: DesignRecipeGroupIndex,
    /// Role-`0x43` aggregate operand-group record.
    pub aggregate_group_record_index: DesignRecipeGroupIndex,
    /// Parameter-owner layout selected by the owned source kinds.
    pub parameter_owners: DesignHemParameterOwners,
    /// Indexed operation-settings record.
    pub settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    pub bend_radius: DesignPositiveScalar,
    /// Byte offset of `bend_radius`.
    pub bend_radius_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignHemOperationWire {
    /// Selection-wrapper record for the hem edge.
    edge_wrapper_record_index: u32,
    /// Role-`0x08` operand-group record.
    edge_group_record_index: u32,
    /// Recipe-backed role-`0x08` operand record.
    edge_operand_record_index: u32,
    /// Role-`0x43` aggregate operand-group record.
    aggregate_group_record_index: u32,
    /// Recipe-backed role-`0x43` operand record.
    aggregate_operand_record_index: u32,
    /// Parameter-owner layout selected by the owned source kinds.
    parameter_owners: DesignHemParameterOwners,
    /// Indexed operation-settings record.
    settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    bend_radius: f64,
    /// Byte offset of `bend_radius`.
    bend_radius_offset: u64,
    form_code: u32,
    direction_code: u32,
    direction_reversal_byte: u8,
    reference_side_code: u32,
}

impl DesignHemOperation {
    pub(crate) fn edge_operand_record_index(&self) -> u32 {
        self.edge_group_record_index.operand()
    }
    pub(crate) fn aggregate_operand_record_index(&self) -> u32 {
        self.aggregate_group_record_index.operand()
    }
}

impl TryFrom<DesignHemOperationWire> for DesignHemOperation {
    type Error = String;

    fn try_from(wire: DesignHemOperationWire) -> Result<Self, Self::Error> {
        if wire.form_code != 3 {
            return Err("form_code must be 3".into());
        }
        if wire.direction_code != 1 {
            return Err("direction_code must be 1".into());
        }
        if wire.direction_reversal_byte != 0 {
            return Err("direction_reversal_byte must be 0".into());
        }
        if wire.reference_side_code != 4 {
            return Err("reference_side_code must be 4".into());
        }
        if Some(wire.edge_operand_record_index) != wire.edge_group_record_index.checked_add(3) {
            return Err("edge_operand_record_index must equal edge_group_record_index + 3".into());
        }
        if Some(wire.aggregate_operand_record_index)
            != wire.aggregate_group_record_index.checked_add(3)
        {
            return Err(
                "aggregate_operand_record_index must equal aggregate_group_record_index + 3".into(),
            );
        }
        Ok(Self {
            edge_wrapper_record_index: wire.edge_wrapper_record_index,
            edge_group_record_index: wire.edge_group_record_index.try_into()?,
            aggregate_group_record_index: wire.aggregate_group_record_index.try_into()?,
            parameter_owners: wire.parameter_owners,
            settings_record_index: wire.settings_record_index,
            bend_radius: DesignPositiveScalar::new(wire.bend_radius)
                .ok_or("bend_radius must be positive and finite")?,
            bend_radius_offset: wire.bend_radius_offset,
        })
    }
}

impl From<DesignHemOperation> for DesignHemOperationWire {
    fn from(record: DesignHemOperation) -> Self {
        Self {
            edge_wrapper_record_index: record.edge_wrapper_record_index,
            edge_group_record_index: record.edge_group_record_index.get(),
            edge_operand_record_index: record.edge_operand_record_index(),
            aggregate_group_record_index: record.aggregate_group_record_index.get(),
            aggregate_operand_record_index: record.aggregate_operand_record_index(),
            parameter_owners: record.parameter_owners,
            settings_record_index: record.settings_record_index,
            bend_radius: record.bend_radius.get(),
            bend_radius_offset: record.bend_radius_offset,
            form_code: 3,
            direction_code: 1,
            direction_reversal_byte: 0,
            reference_side_code: 4,
        }
    }
}

/// Fixed construction carried by a uniform body-scale scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignScaleOperationWire",
    into = "DesignScaleOperationWire"
)]
pub struct DesignScaleOperation {
    /// Counted construction group selecting the transformed bodies.
    pub body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    pub center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    pub center_position: Option<Located<[f64; 3]>>,
    /// Positive uniform scale factor.
    pub uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    pub uniform_factor_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignScaleOperationWire {
    /// Counted construction group selecting the transformed bodies.
    body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    center_position: Option<[f64; 3]>,
    /// Byte offset of the explicit center position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    center_position_offset: Option<u64>,
    /// Positive uniform scale factor.
    uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    uniform_factor_offset: u64,
}

impl From<DesignScaleOperation> for DesignScaleOperationWire {
    fn from(value: DesignScaleOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: value.center_position.map(|center| center.value),
            center_position_offset: value.center_position.map(|center| center.offset),
            uniform_factor: value.uniform_factor,
            uniform_factor_offset: value.uniform_factor_offset,
        }
    }
}

impl TryFrom<DesignScaleOperationWire> for DesignScaleOperation {
    type Error = String;
    fn try_from(value: DesignScaleOperationWire) -> Result<Self, Self::Error> {
        Ok(Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: Located::from_wire(
                value.center_position,
                value.center_position_offset,
                "center_position",
            )?,
            uniform_factor: value.uniform_factor,
            uniform_factor_offset: value.uniform_factor_offset,
        })
    }
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCopyPasteBodiesOperationWire",
    into = "DesignCopyPasteBodiesOperationWire"
)]
pub struct DesignCopyPasteBodiesOperation {
    bodies: Vec<DesignCopiedBody>,
    /// Counted body-selection group named by the scope prefix and reference table.
    pub body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    pub body_group_class_tag: DesignClassTag,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Indexed source-to-copy relation record named by the scope prefix.
    pub relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    pub relation_class_tag: DesignClassTag,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignCopiedBody {
    pub operand: Located<u32>,
    pub source: Located<u32>,
    pub copied: Located<u32>,
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCopyPasteBodiesOperationWire {
    /// Counted body-selection group named by the scope prefix and reference table.
    body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    body_group_class_tag: String,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Ordered body-operand records carried by the counted group.
    body_operand_record_indices: Vec<u32>,
    /// Byte offsets parallel to `body_operand_record_indices`.
    body_operand_record_offsets: Vec<u64>,
    /// Indexed source-to-copy relation record named by the scope prefix.
    relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    relation_class_tag: String,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
    /// Source Design body entity suffixes in copy order.
    source_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `source_body_entity_suffixes`.
    source_body_entity_suffix_offsets: Vec<u64>,
    /// Newly copied Design body entity suffixes parallel to the sources.
    copied_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `copied_body_entity_suffixes`.
    copied_body_entity_suffix_offsets: Vec<u64>,
}

impl DesignCopyPasteBodiesOperation {
    pub(crate) fn try_new(
        bodies: Vec<DesignCopiedBody>,
        body_group_record_index: u32,
        body_group_class_tag: DesignClassTag,
        body_group_byte_offset: u64,
        relation_record_index: u32,
        relation_class_tag: DesignClassTag,
        relation_byte_offset: u64,
    ) -> Result<Self, String> {
        if bodies.is_empty() {
            return Err("bodies must not be empty".into());
        }
        let mut suffixes = std::collections::HashSet::new();
        let mut operand_offset = body_group_byte_offset.saturating_add(26);
        let mut source_offset = relation_byte_offset.saturating_add(25);
        for body in &bodies {
            if !suffixes.insert(body.source.value) || !suffixes.insert(body.copied.value) {
                return Err("source and copied body suffixes must be pairwise distinct".into());
            }
            if body.operand.offset != operand_offset
                || body.source.offset != source_offset
                || body.copied.offset != source_offset.saturating_add(15)
            {
                return Err(
                    "bodies operand, source, and copied offsets must follow their record strides"
                        .into(),
                );
            }
            operand_offset = operand_offset.saturating_add(11);
            source_offset = source_offset.saturating_add(30);
        }
        Ok(Self {
            bodies,
            body_group_record_index,
            body_group_class_tag,
            body_group_byte_offset,
            relation_record_index,
            relation_class_tag,
            relation_byte_offset,
        })
    }
    pub(crate) fn bodies(&self) -> &[DesignCopiedBody] {
        &self.bodies
    }
    pub(crate) fn body_group_byte_offset(&self) -> u64 {
        self.body_group_byte_offset
    }
    pub(crate) fn relation_byte_offset(&self) -> u64 {
        self.relation_byte_offset
    }
}

impl TryFrom<DesignCopyPasteBodiesOperationWire> for DesignCopyPasteBodiesOperation {
    type Error = String;
    fn try_from(wire: DesignCopyPasteBodiesOperationWire) -> Result<Self, Self::Error> {
        let count = wire.body_operand_record_indices.len();
        if wire.body_operand_record_offsets.len() != count {
            return Err(
                "body_operand_record_offsets must match body_operand_record_indices".into(),
            );
        }
        if wire.source_body_entity_suffixes.len() != count {
            return Err(
                "source_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.source_body_entity_suffix_offsets.len() != count {
            return Err(
                "source_body_entity_suffix_offsets must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffixes.len() != count {
            return Err(
                "copied_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffix_offsets.len() != count {
            return Err(
                "copied_body_entity_suffix_offsets must match body_operand_record_indices".into(),
            );
        }
        let bodies = wire
            .body_operand_record_indices
            .into_iter()
            .zip(wire.body_operand_record_offsets)
            .zip(
                wire.source_body_entity_suffixes
                    .into_iter()
                    .zip(wire.source_body_entity_suffix_offsets),
            )
            .zip(
                wire.copied_body_entity_suffixes
                    .into_iter()
                    .zip(wire.copied_body_entity_suffix_offsets),
            )
            .map(
                |(((value, offset), (source, source_offset)), (copied, copied_offset))| {
                    DesignCopiedBody {
                        operand: Located { value, offset },
                        source: Located {
                            value: source,
                            offset: source_offset,
                        },
                        copied: Located {
                            value: copied,
                            offset: copied_offset,
                        },
                    }
                },
            )
            .collect();
        Self::try_new(
            bodies,
            wire.body_group_record_index,
            wire.body_group_class_tag.try_into()?,
            wire.body_group_byte_offset,
            wire.relation_record_index,
            wire.relation_class_tag.try_into()?,
            wire.relation_byte_offset,
        )
    }
}
impl From<DesignCopyPasteBodiesOperation> for DesignCopyPasteBodiesOperationWire {
    fn from(value: DesignCopyPasteBodiesOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            body_group_class_tag: value.body_group_class_tag.into(),
            body_group_byte_offset: value.body_group_byte_offset,
            relation_record_index: value.relation_record_index,
            relation_class_tag: value.relation_class_tag.into(),
            relation_byte_offset: value.relation_byte_offset,
            body_operand_record_indices: value
                .bodies
                .iter()
                .map(|body| body.operand.value)
                .collect(),
            body_operand_record_offsets: value
                .bodies
                .iter()
                .map(|body| body.operand.offset)
                .collect(),
            source_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.source.value)
                .collect(),
            source_body_entity_suffix_offsets: value
                .bodies
                .iter()
                .map(|body| body.source.offset)
                .collect(),
            copied_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.copied.value)
                .collect(),
            copied_body_entity_suffix_offsets: value
                .bodies
                .iter()
                .map(|body| body.copied.offset)
                .collect(),
        }
    }
}

/// Encoded compact Base Feature mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DesignBaseFeatureCompactMode {
    Zero = 0,
    One = 1,
}

impl TryFrom<u8> for DesignBaseFeatureCompactMode {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            _ => Err("mode must be 0 or 1"),
        }
    }
}

/// Layout of the legacy class-452/class-262 Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignBaseFeatureBodyReferenceForm {
    /// One output body with an encoded compact mode.
    CompactOneBody {
        mode: Located<DesignBaseFeatureCompactMode>,
        body: DesignLegacyBaseFeatureBody,
    },
    /// Two output bodies with no mode slot.
    ExpandedTwoBody {
        bodies: [DesignLegacyBaseFeatureBody; 2],
    },
}

/// One body in a legacy Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignLegacyBaseFeatureBody {
    pub entity: DesignBaseFeatureEntry<u32>,
    pub parameter_body: Located<u64>,
    pub auxiliary: Located<u64>,
}

impl DesignBaseFeatureBodyReferenceForm {
    fn bodies(&self) -> &[DesignLegacyBaseFeatureBody] {
        match self {
            Self::CompactOneBody { body, .. } => std::slice::from_ref(body),
            Self::ExpandedTwoBody { bodies } => bodies,
        }
    }
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignBaseFeatureConstructionWire",
    into = "DesignBaseFeatureConstructionWire"
)]
pub enum DesignBaseFeatureConstruction {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered body, passive-reference, result, and optional repeated-field rows.
        bodies: DesignBaseFeatureResults,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of the shared metadata record.
        metadata_record_offset: u64,
        /// Variant-width source field following the metadata record.
        metadata_field: Vec<u8>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The single body suffix and the location shared by its reference views.
        body: Located<u32>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceForm,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered snapshot bodies with their source fields.
        bodies: Vec<DesignBaseFeatureEntry<u64>>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [DesignRelaxedGuidText; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
}

/// One aligned body, passive reference, and result record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignBaseFeatureResultBody {
    pub entity: DesignBaseFeatureEntry<u64>,
    pub reference: DesignBaseFeatureEntry<u32>,
    pub result: DesignBaseFeatureEntry<u32>,
}

/// Result-body runs with either no repeated fields or one field per body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignBaseFeatureResults {
    WithoutRepeatedFields(Vec<DesignBaseFeatureResultBody>),
    WithRepeatedFields {
        first: (DesignBaseFeatureResultBody, [u8; 6]),
        rest: Vec<(DesignBaseFeatureResultBody, [u8; 6])>,
    },
}

impl DesignBaseFeatureResults {
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &DesignBaseFeatureResultBody> {
        let count = match self {
            Self::WithoutRepeatedFields(bodies) => bodies.len(),
            Self::WithRepeatedFields { rest, .. } => 1 + rest.len(),
        };
        (0..count).map(move |index| match self {
            Self::WithoutRepeatedFields(bodies) => &bodies[index],
            Self::WithRepeatedFields { first, .. } if index == 0 => &first.0,
            Self::WithRepeatedFields { rest, .. } => &rest[index - 1].0,
        })
    }
}

/// One Base Feature reference value, its location, and its six-byte field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignBaseFeatureEntry<T> {
    pub value: T,
    pub offset: u64,
    pub field: [u8; 6],
}

/// Wire form of the legacy class-452/class-262 Base Feature body-reference
/// envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesignBaseFeatureBodyReferenceFormWire {
    /// One output body with 64-bit references in the legacy compact lanes.
    CompactOneBody,
    /// Two output bodies with counted 32-bit reference runs.
    ExpandedTwoBody,
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Untagged: required field sets are disjoint across variants.
#[serde(untagged)]
enum DesignBaseFeatureConstructionWire {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered Design body entity suffixes exposed by the Base Feature.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Ordered passive body-reference records parallel to the body suffixes.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_reference_records`.
        body_reference_fields: Vec<[u8; 6]>,
        /// Six-byte source fields in the repeated passive-reference run.
        repeated_reference_fields: Vec<[u8; 6]>,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of `metadata_record`.
        metadata_record_offset: u64,
        /// Variant-width source field following `metadata_record`.
        metadata_field: Vec<u8>,
        /// Ordered result-body join records parallel to the body suffixes.
        result_records: Vec<u32>,
        /// Byte offsets parallel to `result_records`.
        result_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `result_records`.
        result_fields: Vec<[u8; 6]>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The Design body entity suffix exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceFormWire,
        /// Compact-form mode byte. The expanded form has no mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u8>,
        /// Byte offset of the compact-form mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode_offset: Option<u64>,
        /// Ordered Design body entity suffixes exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Ordered PM body-reference records carried by the envelope.
        parameter_body_records: Vec<u64>,
        /// Byte offsets parallel to `parameter_body_records`.
        parameter_body_record_offsets: Vec<u64>,
        /// Ordered DM body-reference records carried by the envelope.
        auxiliary_records: Vec<u64>,
        /// Byte offsets parallel to `auxiliary_records`.
        auxiliary_record_offsets: Vec<u64>,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered Design body entity suffixes exposed by the snapshot.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [DesignRelaxedGuidText; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
}

impl TryFrom<DesignBaseFeatureConstructionWire> for DesignBaseFeatureConstruction {
    type Error = String;
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn try_from(wire: DesignBaseFeatureConstructionWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignBaseFeatureConstructionWire::ResultBodies {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                body_reference_records,
                body_reference_record_offsets,
                body_reference_fields,
                repeated_reference_fields,
                metadata_record,
                metadata_record_offset,
                metadata_field,
                result_records,
                result_record_offsets,
                result_fields,
            } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    (
                        "body_entity_suffix_offsets",
                        body_entity_suffix_offsets.len(),
                    ),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    (
                        "body_reference_record_offsets",
                        body_reference_record_offsets.len(),
                    ),
                    ("body_reference_fields", body_reference_fields.len()),
                    ("result_records", result_records.len()),
                    ("result_record_offsets", result_record_offsets.len()),
                    ("result_fields", result_fields.len()),
                ] {
                    if len != count {
                        return Err(format!(
                            "{field} must have the same length as body_entity_suffixes"
                        ));
                    }
                }
                if !repeated_reference_fields.is_empty() && repeated_reference_fields.len() != count
                {
                    return Err("repeated_reference_fields must be empty or have the same length as body_entity_suffixes".into());
                }
                let bodies = (0..count).map(|index| DesignBaseFeatureResultBody {
                    entity: DesignBaseFeatureEntry {
                        value: body_entity_suffixes[index],
                        offset: body_entity_suffix_offsets[index],
                        field: body_entity_fields[index],
                    },
                    reference: DesignBaseFeatureEntry {
                        value: body_reference_records[index],
                        offset: body_reference_record_offsets[index],
                        field: body_reference_fields[index],
                    },
                    result: DesignBaseFeatureEntry {
                        value: result_records[index],
                        offset: result_record_offsets[index],
                        field: result_fields[index],
                    },
                });
                let bodies = if repeated_reference_fields.is_empty() {
                    DesignBaseFeatureResults::WithoutRepeatedFields(bodies.collect())
                } else {
                    let mut repeated = bodies.zip(repeated_reference_fields);
                    match repeated.next() {
                        Some(first) => DesignBaseFeatureResults::WithRepeatedFields {
                            first,
                            rest: repeated.collect(),
                        },
                        None => {
                            return Err(
                                "repeated_reference_fields require body_entity_suffixes".into()
                            )
                        }
                    }
                };
                Self::ResultBodies {
                    bodies,
                    metadata_record,
                    metadata_record_offset,
                    metadata_field,
                }
            }
            DesignBaseFeatureConstructionWire::BodyBasedOnFaces {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_reference_records,
                body_reference_record_offsets,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces,
                tag_body_based_on_faces_offset,
            } => {
                let ([suffix], [offset], [reference], [reference_offset]) = (
                    body_entity_suffixes.as_slice(),
                    body_entity_suffix_offsets.as_slice(),
                    body_reference_records.as_slice(),
                    body_reference_record_offsets.as_slice(),
                ) else {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, body_reference_records, and body_reference_record_offsets require one body".into());
                };
                if *suffix != u64::from(*reference) || offset != reference_offset {
                    return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                }
                if !tag_body_based_on_faces {
                    return Err("tag_body_based_on_faces must be true for BodyBasedOnFaces".into());
                }
                Self::BodyBasedOnFaces {
                    body: Located {
                        value: *reference,
                        offset: *offset,
                    },
                    parameter_body_record,
                    parameter_body_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstructionWire::LegacyBodyBasedOnFaces {
                form,
                mode,
                mode_offset,
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                body_reference_records,
                body_reference_record_offsets,
                parameter_body_records,
                parameter_body_record_offsets,
                auxiliary_records,
                auxiliary_record_offsets,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces,
                tag_body_based_on_faces_offset,
            } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    (
                        "body_entity_suffix_offsets",
                        body_entity_suffix_offsets.len(),
                    ),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    (
                        "body_reference_record_offsets",
                        body_reference_record_offsets.len(),
                    ),
                    ("parameter_body_records", parameter_body_records.len()),
                    (
                        "parameter_body_record_offsets",
                        parameter_body_record_offsets.len(),
                    ),
                    ("auxiliary_records", auxiliary_records.len()),
                    ("auxiliary_record_offsets", auxiliary_record_offsets.len()),
                ] {
                    if len != count {
                        return Err(format!(
                            "{field} must have the same length as body_entity_suffixes"
                        ));
                    }
                }
                if !tag_body_based_on_faces {
                    return Err(
                        "tag_body_based_on_faces must be true for LegacyBodyBasedOnFaces".into(),
                    );
                }
                let mut bodies = Vec::with_capacity(count);
                for index in 0..count {
                    if body_entity_suffixes[index] != u64::from(body_reference_records[index])
                        || body_entity_suffix_offsets[index] != body_reference_record_offsets[index]
                    {
                        return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                    }
                    bodies.push(DesignLegacyBaseFeatureBody {
                        entity: DesignBaseFeatureEntry {
                            value: body_reference_records[index],
                            offset: body_entity_suffix_offsets[index],
                            field: body_entity_fields[index],
                        },
                        parameter_body: Located {
                            value: parameter_body_records[index],
                            offset: parameter_body_record_offsets[index],
                        },
                        auxiliary: Located {
                            value: auxiliary_records[index],
                            offset: auxiliary_record_offsets[index],
                        },
                    });
                }
                let form = match (form, mode, mode_offset, bodies.as_slice()) {
                    (DesignBaseFeatureBodyReferenceFormWire::CompactOneBody, Some(value), Some(offset), [body]) => DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode: Located { value: DesignBaseFeatureCompactMode::try_from(value)?, offset }, body: *body },
                    (DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody, None, None, [first, second]) => DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { bodies: [*first, *second] },
                    _ => return Err("form requires one body with mode and mode_offset for compact_one_body, or two bodies without mode for expanded_two_body".into()),
                };
                Self::LegacyBodyBasedOnFaces {
                    form,
                    scope_reference,
                    scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstructionWire::BodySnapshot {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => {
                if body_entity_suffixes.len() != body_entity_suffix_offsets.len()
                    || body_entity_suffixes.len() != body_entity_fields.len()
                {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, and body_entity_fields must have equal lengths".into());
                }
                let bodies = body_entity_suffixes
                    .into_iter()
                    .zip(body_entity_suffix_offsets)
                    .zip(body_entity_fields)
                    .map(|((value, offset), field)| DesignBaseFeatureEntry {
                        value,
                        offset,
                        field,
                    })
                    .collect();
                Self::BodySnapshot {
                    bodies,
                    related_guids,
                    related_guid_offsets,
                    linkage_record,
                    linkage_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                }
            }
        })
    }
}

impl From<DesignBaseFeatureConstruction> for DesignBaseFeatureConstructionWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(value: DesignBaseFeatureConstruction) -> Self {
        match value {
            DesignBaseFeatureConstruction::ResultBodies {
                bodies,
                metadata_record,
                metadata_record_offset,
                metadata_field,
            } => {
                let body_entity_suffixes = bodies.iter().map(|body| body.entity.value).collect();
                let body_entity_suffix_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies.iter().map(|body| body.entity.field).collect();
                let body_reference_records =
                    bodies.iter().map(|body| body.reference.value).collect();
                let body_reference_record_offsets =
                    bodies.iter().map(|body| body.reference.offset).collect();
                let body_reference_fields =
                    bodies.iter().map(|body| body.reference.field).collect();
                let result_records = bodies.iter().map(|body| body.result.value).collect();
                let result_record_offsets = bodies.iter().map(|body| body.result.offset).collect();
                let result_fields = bodies.iter().map(|body| body.result.field).collect();
                let repeated_reference_fields = match bodies {
                    DesignBaseFeatureResults::WithoutRepeatedFields(_) => Vec::new(),
                    DesignBaseFeatureResults::WithRepeatedFields { first, rest } => {
                        std::iter::once(first.1)
                            .chain(rest.into_iter().map(|(_, field)| field))
                            .collect()
                    }
                };
                Self::ResultBodies {
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    body_reference_records,
                    body_reference_record_offsets,
                    body_reference_fields,
                    repeated_reference_fields,
                    metadata_record,
                    metadata_record_offset,
                    metadata_field,
                    result_records,
                    result_record_offsets,
                    result_fields,
                }
            }
            DesignBaseFeatureConstruction::BodyBasedOnFaces {
                body,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => Self::BodyBasedOnFaces {
                body_entity_suffixes: vec![u64::from(body.value)],
                body_entity_suffix_offsets: vec![body.offset],
                body_reference_records: vec![body.value],
                body_reference_record_offsets: vec![body.offset],
                tag_body_based_on_faces: true,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            },
            DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
                form,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => {
                let bodies = form.bodies();
                let body_entity_suffixes = bodies
                    .iter()
                    .map(|body| u64::from(body.entity.value))
                    .collect();
                let body_entity_suffix_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies.iter().map(|body| body.entity.field).collect();
                let body_reference_records = bodies.iter().map(|body| body.entity.value).collect();
                let body_reference_record_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let parameter_body_records = bodies
                    .iter()
                    .map(|body| body.parameter_body.value)
                    .collect();
                let parameter_body_record_offsets = bodies
                    .iter()
                    .map(|body| body.parameter_body.offset)
                    .collect();
                let auxiliary_records = bodies.iter().map(|body| body.auxiliary.value).collect();
                let auxiliary_record_offsets =
                    bodies.iter().map(|body| body.auxiliary.offset).collect();
                let (form, mode, mode_offset) = match form {
                    DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::CompactOneBody,
                        Some(mode.value as u8),
                        Some(mode.offset),
                    ),
                    DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody,
                        None,
                        None,
                    ),
                };
                Self::LegacyBodyBasedOnFaces {
                    form,
                    mode,
                    mode_offset,
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    body_reference_records,
                    body_reference_record_offsets,
                    parameter_body_records,
                    parameter_body_record_offsets,
                    auxiliary_records,
                    auxiliary_record_offsets,
                    scope_reference,
                    scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces: true,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstruction::BodySnapshot {
                bodies,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => {
                let mut body_entity_suffixes = Vec::with_capacity(bodies.len());
                let mut body_entity_suffix_offsets = Vec::with_capacity(bodies.len());
                let mut body_entity_fields = Vec::with_capacity(bodies.len());
                for body in bodies {
                    body_entity_suffixes.push(body.value);
                    body_entity_suffix_offsets.push(body.offset);
                    body_entity_fields.push(body.field);
                }
                Self::BodySnapshot {
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    related_guids,
                    related_guid_offsets,
                    linkage_record,
                    linkage_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                }
            }
        }
    }
}

impl DesignBaseFeatureConstruction {
    /// Return the body suffixes in source order for any Base Feature form.
    pub(crate) fn body_entity_suffixes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        let count = match self {
            Self::BodySnapshot { bodies, .. } => bodies.len(),
            Self::BodyBasedOnFaces { .. } => 1,
            Self::ResultBodies { bodies, .. } => bodies.iter().len(),
            Self::LegacyBodyBasedOnFaces { form, .. } => form.bodies().len(),
        };
        (0..count).map(move |index| match self {
            Self::BodySnapshot { bodies, .. } => bodies[index].value,
            Self::BodyBasedOnFaces { body, .. } => u64::from(body.value),
            Self::ResultBodies { bodies, .. } => match bodies {
                DesignBaseFeatureResults::WithoutRepeatedFields(bodies) => {
                    bodies[index].entity.value
                }
                DesignBaseFeatureResults::WithRepeatedFields { first, .. } if index == 0 => {
                    first.0.entity.value
                }
                DesignBaseFeatureResults::WithRepeatedFields { rest, .. } => {
                    rest[index - 1].0.entity.value
                }
            },
            Self::LegacyBodyBasedOnFaces { form, .. } => {
                u64::from(form.bodies()[index].entity.value)
            }
        })
    }

    /// Return passive body-reference records for forms that carry them.
    pub(crate) fn body_reference_records(&self) -> impl Iterator<Item = u32> + '_ {
        let (results, legacy, single) = match self {
            Self::ResultBodies { bodies, .. } => (Some(bodies), &[][..], None),
            Self::LegacyBodyBasedOnFaces { form, .. } => (None, form.bodies(), None),
            Self::BodyBasedOnFaces { body, .. } => (None, &[][..], Some(body.value)),
            Self::BodySnapshot { .. } => (None, &[][..], None),
        };
        results
            .into_iter()
            .flat_map(DesignBaseFeatureResults::iter)
            .map(|body| body.reference.value)
            .chain(legacy.iter().map(|body| body.entity.value))
            .chain(single)
    }
}

impl DesignWorkPointInput {
    pub(crate) fn try_set_carrier(
        &mut self,
        carrier: Option<Box<DesignWorkPointInputCarrier>>,
    ) -> Result<(), String> {
        let mut draft = self.clone().into_draft();
        draft.carrier = carrier;
        *self = Self::try_new(draft)?;
        Ok(())
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
