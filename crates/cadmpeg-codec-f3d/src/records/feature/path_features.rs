// SPDX-License-Identifier: Apache-2.0
//! Path features: revolve, loft, sweep and pipe constructions.

use super::extrude::DesignExtrudeOperation;
use crate::records::identity::Sourced;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::{FiniteReal, PositiveAngle};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_opposite_angle_record_index,
    u32,
    "opposite_angle_record_index"
);
/// Exact construction carried by a Revolve, Loft, or Sweep scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignPathFeatureConstruction {
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
}

impl From<DesignRevolveConstruction> for DesignRevolveConstructionWire {
    fn from(value: DesignRevolveConstruction) -> Self {
        Self {
            operation: value.operation,
            angle: value.angle.get(),
            angle_record_index: value.angle_record_index,
            opposite_angle_record_index: value.opposite_angle.map(|located| located.value),
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
            opposite_angle: value.opposite_angle_record_index.map(|value| Sourced {
                value,
                offset: None,
            }),
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
    /// Six scalar values in `AlongDistance`, `AgainstDistance`,
    /// `AlongRailDistance`, `AgainstRailDistance`, `TwistAngle`, and `TaperAngle` order.
    pub(crate) values: [FiniteReal; 6],
    /// Referenced scalar records in lane order.
    pub(crate) record_indexes: [u32; 6],
    /// Byte offsets of the scalar values in lane order of a located scope.
    #[serde(skip)]
    pub(crate) value_offsets: Option<[u64; 6]>,
}

/// Fixed construction of a `Pipe` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Four scalar values in path-fraction, reverse-path-fraction,
    /// section-size, and section-thickness order.
    pub(crate) values: [FiniteReal; 4],
    /// Referenced scalar records in lane order.
    pub(crate) record_indexes: [u32; 4],
    /// Byte offsets of the scalar values in lane order of a located scope.
    #[serde(skip)]
    pub(crate) value_offsets: Option<[u64; 4]>,
}

mod identity_rewrite;
