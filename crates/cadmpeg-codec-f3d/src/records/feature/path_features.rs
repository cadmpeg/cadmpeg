// SPDX-License-Identifier: Apache-2.0
//! Path features: revolve, loft, sweep and pipe constructions.

use super::extrude::DesignExtrudeOperation;
use super::work_geometry::DesignWorkPointConstruction;
use crate::records::identity::Sourced;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::{FiniteReal, PositiveAngle};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_opposite_angle_record_index,
    u32,
    "opposite_angle_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_second_side,
    DesignRevolveSecondSideWire,
    "second_side"
);
/// Exact construction carried by a Revolve, Loft, Sweep, or Pipe scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignPathFeatureConstruction {
    /// Fixed-angle revolution result operation.
    Revolve(DesignRevolveConstruction),
    /// Loft result operation.
    Loft(DesignLoftConstruction),
    /// Sweep result operation and fixed dimension lanes.
    Sweep(DesignSweepConstruction),
    /// Native Pipe prefix and canonical scalar-owner identities.
    Pipe(DesignPipeConstruction),
}

/// Fixed construction of a `Revolve` scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRevolveConstructionWire",
    into = "DesignRevolveConstructionWire"
)]
pub(crate) struct DesignRevolveConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32 of a located scope.
    pub(crate) operation_offset: Option<u64>,
    /// Positive angular travel in radians.
    pub(crate) angle: PositiveAngle,
    /// Referenced angular-travel scalar record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the angular-travel scalar of a located scope.
    pub(crate) angle_offset: Option<u64>,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) opposite_angle: Option<Sourced<u32>>,
    /// Travel opposite the revolution direction, when the scope revolves to
    /// both sides of the profile plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) second_side: Option<DesignRevolveSecondSide>,
}

/// Opposite-side travel of a Symmetric or Two Sides `Revolve` scope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignRevolveSecondSide {
    /// Whether the scope mirrors the first side (Symmetric) rather than
    /// storing an independent second angle (Two Sides).
    pub(crate) symmetric: bool,
    /// Positive angular travel opposite the revolution direction, in radians.
    pub(crate) angle: PositiveAngle,
    /// Referenced opposite-side angle owner record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the opposite-side angular travel of a located scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) angle_offset: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct DesignRevolveSecondSideWire {
    symmetric: bool,
    angle: f64,
    angle_record_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle_offset: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct DesignRevolveConstructionWire {
    /// Boolean result operation.
    operation: DesignExtrudeOperation,
    /// Positive angular travel in radians.
    angle: f64,
    /// Referenced angular-travel scalar record.
    angle_record_index: u32,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle_record_index"
    )]
    opposite_angle_record_index: Option<u32>,
    /// Byte offset of the opposite-side angle scalar, when serialized.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle_offset"
    )]
    opposite_angle_offset: Option<u64>,
    /// Opposite-side travel of a Symmetric or Two Sides scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_side"
    )]
    second_side: Option<DesignRevolveSecondSideWire>,
}

impl From<DesignRevolveConstruction> for DesignRevolveConstructionWire {
    fn from(value: DesignRevolveConstruction) -> Self {
        Self {
            operation: value.operation,
            angle: value.angle.get(),
            angle_record_index: value.angle_record_index,
            opposite_angle_record_index: value.opposite_angle.map(|located| located.value),
            opposite_angle_offset: value.opposite_angle.and_then(|sourced| sourced.offset),
            second_side: value.second_side.map(|side| DesignRevolveSecondSideWire {
                symmetric: side.symmetric,
                angle: side.angle.get(),
                angle_record_index: side.angle_record_index,
                angle_offset: side.angle_offset,
            }),
        }
    }
}

impl TryFrom<DesignRevolveConstructionWire> for DesignRevolveConstruction {
    type Error = String;
    fn try_from(value: DesignRevolveConstructionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            operation: value.operation,
            operation_offset: None,
            angle: PositiveAngle::new(value.angle).ok_or("angle must be positive and finite")?,
            angle_record_index: value.angle_record_index,
            angle_offset: None,
            opposite_angle: match (
                value.opposite_angle_record_index,
                value.opposite_angle_offset,
            ) {
                (None, None) => None,
                (Some(value), Some(offset)) => Some(Sourced::at(value, offset)),
                _ => {
                    return Err(
                        "opposite_angle_record_index and opposite_angle_offset must occur together"
                            .into(),
                    )
                }
            },
            second_side: value
                .second_side
                .map(|side| {
                    Ok::<_, String>(DesignRevolveSecondSide {
                        symmetric: side.symmetric,
                        angle: PositiveAngle::new(side.angle)
                            .ok_or("second-side angle must be positive and finite")?,
                        angle_record_index: side.angle_record_index,
                        angle_offset: side.angle_offset,
                    })
                })
                .transpose()?,
        })
    }
}

/// Fixed construction of a `Loft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignLoftConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32 of a located scope.
    #[serde(skip)]
    pub(crate) operation_offset: Option<u64>,
}

/// Fixed construction of a `Sweep` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSweepConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32 of a located scope.
    #[serde(skip)]
    pub(crate) operation_offset: Option<u64>,
    /// Scalar values in lane order: `AlongDistance` and `AgainstDistance`,
    /// followed by `AlongRailDistance`, `AgainstRailDistance`, `TwistAngle`, and
    /// `TaperAngle` when present.
    pub(crate) values: Vec<FiniteReal>,
    /// Referenced scalar records in lane order.
    pub(crate) record_indexes: Vec<u32>,
    /// Byte offsets of the scalar values in lane order of a located scope.
    #[serde(skip)]
    pub(crate) value_offsets: Option<Vec<u64>>,
}

/// Native construction of a `Pipe` scope.
///
/// The four scalar values live on the canonical parameter-owner records named
/// by `record_indexes`; the construction keeps only their identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignPipeConstructionWire")]
pub(crate) struct DesignPipeConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32 of a located scope.
    #[serde(skip)]
    pub(crate) operation_offset: Option<u64>,
    /// Whether the section has an inward hollow wall.
    pub(crate) is_hollow: bool,
    /// Byte offset of the hollow Boolean of a located scope.
    #[serde(skip)]
    pub(crate) is_hollow_offset: Option<u64>,
    /// Source bytes following the result operation.
    pub(crate) opaque_post_operation: NativeBytes<[u8; 2]>,
    /// Byte offset of the source pair following the result operation of a located scope.
    #[serde(skip)]
    pub(crate) opaque_post_operation_offset: Option<u64>,
    /// Canonical scalar owners in forward-fraction, reverse-fraction,
    /// Section Size, and Section Thickness order.
    pub(crate) record_indexes: [u32; 4],
    /// Optional native point-selection pair.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) point_selection: Option<DesignPipePointSelection>,
}

/// Point-data record and its recipe-selection input named by a `Pipe` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignPipePointSelection {
    /// Offset of the marked prefix reference that names the point-data record
    /// of a located scope.
    #[serde(skip)]
    pub(crate) prefix_reference_offset: Option<u64>,
    /// Complete point-data construction.
    pub(crate) point_data: DesignWorkPointConstruction,
}

cadmpeg_core::named_optional_field!(
    deserialize_pipe_point_selection,
    DesignPipePointSelection,
    "point_selection"
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignPipeConstructionWire {
    operation: DesignExtrudeOperation,
    is_hollow: bool,
    opaque_post_operation: NativeBytes<[u8; 2]>,
    record_indexes: [u32; 4],
    #[serde(default, deserialize_with = "deserialize_pipe_point_selection")]
    point_selection: Option<DesignPipePointSelection>,
}

impl TryFrom<DesignPipeConstructionWire> for DesignPipeConstruction {
    type Error = &'static str;

    fn try_from(value: DesignPipeConstructionWire) -> Result<Self, Self::Error> {
        if value
            .record_indexes
            .iter()
            .enumerate()
            .any(|(ordinal, id)| *id == 0 || value.record_indexes[..ordinal].contains(id))
        {
            return Err("invalid Pipe scalar identities");
        }
        Ok(Self {
            operation: value.operation,
            operation_offset: None,
            is_hollow: value.is_hollow,
            is_hollow_offset: None,
            opaque_post_operation: value.opaque_post_operation,
            opaque_post_operation_offset: None,
            record_indexes: value.record_indexes,
            point_selection: value.point_selection,
        })
    }
}

impl DesignPipeConstruction {
    /// Whether the prefix fields of a located scope sit at the compact or
    /// legacy offsets of a scope starting at `start`, before its reference
    /// table at `table`.
    pub(super) fn scope_offsets_are_valid(&self, start: u64, table: u64) -> bool {
        use crate::layout::{
            fixed_pipe_operation_prefix as compact, legacy_pipe_operation_prefix as legacy,
        };
        use cadmpeg_core::decode::u64_from_index;
        let (Some(is_hollow_offset), Some(operation_offset), Some(post_offset)) = (
            self.is_hollow_offset,
            self.operation_offset,
            self.opaque_post_operation_offset,
        ) else {
            return false;
        };
        let compact = start.checked_add(u64_from_index(compact::IS_HOLLOW));
        let legacy = start.checked_add(u64_from_index(legacy::IS_HOLLOW));
        let prefix_end = post_offset.checked_add(2);
        [compact, legacy].contains(&Some(is_hollow_offset))
            && is_hollow_offset.checked_add(1) == Some(operation_offset)
            && operation_offset.checked_add(4) == Some(post_offset)
            && prefix_end.is_some_and(|end| end <= table)
            && self.point_selection.as_ref().is_none_or(|selection| {
                selection.prefix_reference_offset.is_some_and(|reference| {
                    prefix_end.is_some_and(|end| reference > end)
                        && reference.checked_add(10).is_some_and(|end| end <= table)
                })
            })
    }
}

mod identity_rewrite;
pub(crate) mod pipe;
