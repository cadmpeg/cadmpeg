// SPDX-License-Identifier: Apache-2.0
//! Extrude operations, extents, starts and the extrude prologue.

use crate::records::identity::Sourced;
use crate::records::source_offsets::{OffsetVisitor, SourceOffsets};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_operation_prefix_marker,
    u8,
    "operation_prefix_marker"
);
/// Boolean result operation stored by an Extrude parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignExtrudeOperation {
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
pub(crate) enum DesignExtrudeExtent {
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
pub(crate) enum DesignExtrudeStart {
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
pub(crate) struct DesignExtrudePrologueReference {
    /// Referenced Design record.
    pub(crate) record_index: u32,
    /// Byte offset of `record_index`.
    pub(crate) record_index_offset: Option<u64>,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    pub(crate) trailing_zero_count: u8,
    /// The marker 1 before the operation, when the prologue stores it.
    pub(crate) operation_prefix_marker: Option<Sourced<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct DesignExtrudePrologueReferenceWire {
    /// Referenced Design record.
    record_index: u32,
    /// Number of zero bytes between `record_index` and the operation or its marker.
    trailing_zero_count: u8,
    /// Optional marker byte between the zero run and the operation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operation_prefix_marker"
    )]
    operation_prefix_marker: Option<u8>,
}

/// The stored operation-prefix marker, which is the value 1 when present.
fn operation_prefix_marker(marker: Option<u8>) -> Result<Option<Sourced<()>>, String> {
    match marker {
        None => Ok(None),
        Some(1) => Ok(Some(Sourced {
            value: (),
            offset: None,
        })),
        Some(_) => Err("operation_prefix_marker must be 1".into()),
    }
}

impl TryFrom<DesignExtrudePrologueReferenceWire> for DesignExtrudePrologueReference {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueReferenceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            record_index_offset: None,
            trailing_zero_count: wire.trailing_zero_count,
            operation_prefix_marker: operation_prefix_marker(wire.operation_prefix_marker)?,
        })
    }
}

impl From<DesignExtrudePrologueReference> for DesignExtrudePrologueReferenceWire {
    fn from(record: DesignExtrudePrologueReference) -> Self {
        Self {
            record_index: record.record_index,
            trailing_zero_count: record.trailing_zero_count,
            operation_prefix_marker: record.operation_prefix_marker.map(|_| 1),
        }
    }
}

/// Scope-reference ordinal repeated before a whole-body Extrude target extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeTargetOrdinal {
    /// Zero-based ordinal in the enclosing scope reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Byte offset of `scope_reference_ordinal`.
    #[serde(skip)]
    pub(crate) scope_reference_ordinal_offset: Option<u64>,
}

/// Fixed fields preceding an Extrude parameter scope's reference table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignExtrudePrologueWire",
    into = "DesignExtrudePrologueWire"
)]
pub(crate) enum DesignExtrudePrologue {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// The fixed zero prefix, when its marker is present.
        prefix_zero: Option<Sourced<()>>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: Option<u64>,
        /// Byte offset of the fixed extent-kind value `2`.
        extent_kind_offset: Option<u64>,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: Option<u64>,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of the geometry-kind integer.
        solid_operation_offset: Option<u64>,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: Option<u64>,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        side_extent_discriminator_offsets: Option<[u64; 2]>,
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        direction_face_extend_offsets: Option<[u64; 2]>,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: Option<u64>,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: Option<u64>,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: Option<u64>,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: Option<u64>,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        side_extent_discriminator_offsets: Option<[u64; 2]>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Byte offsets parallel to `direction_face_extend_values`.
        direction_face_extend_offsets: Option<[u64; 2]>,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: Option<u64>,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: Option<u64>,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: Option<u64>,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// The optional marker 1 before the operation.
        operation_prefix_marker: Option<Sourced<()>>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Byte offset of `operation`.
        operation_offset: Option<u64>,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Byte offsets parallel to `side_extent_discriminators`.
        side_extent_discriminator_offsets: Option<[u64; 2]>,
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Byte offsets parallel to `direction_face_extend_values`.
        direction_face_extend_offsets: Option<[u64; 2]>,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Byte offset of `direction_reversed`.
        direction_reversed_offset: Option<u64>,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Byte offset of `solid_operation`.
        solid_operation_offset: Option<u64>,
        /// Starting support.
        start: DesignExtrudeStart,
        /// Byte offset of `start`.
        start_offset: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "layout")]
enum DesignExtrudePrologueWire {
    /// Early distance-only layout with a nullable prefix field.
    LegacyDistance {
        /// Value of the nullable prefix field when its marker is present.
        prefix_value: Option<u32>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Raw extent-kind value (`2 = one-sided distance`).
        #[serde(alias = "extent_discriminator")]
        extent_kind: u32,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Raw geometry-kind discriminator (`0 = sheet`, `1 = solid`).
        geometry_kind: u32,
    },
    /// Reference-aware layout with an optional indexed-reference prefix.
    ReferenceAware {
        /// Indexed-record prefix, when present.
        reference: Option<DesignExtrudePrologueReference>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored after the profile normal and reference slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Repeated target-group ordinal in the whole-body target form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_side_target_ordinal: Option<DesignExtrudeTargetOrdinal>,
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Starting support.
        start: DesignExtrudeStart,
    },
    /// Shifted reference-aware two-sided face-target layout.
    ShiftedReferenceAware {
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side extent discriminators stored in the fixed legacy tail.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Decoded extent form.
        extent: DesignExtrudeExtent,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Starting support.
        start: DesignExtrudeStart,
    },
    /// Shifted layout without the reference-aware prefix.
    LegacyShifted {
        /// Optional marker immediately before the operation fields.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_prefix_marker: Option<u8>,
        /// Boolean result operation.
        operation: DesignExtrudeOperation,
        /// Raw travel-direction and face-extension values.
        #[serde(alias = "extent_discriminators")]
        direction_face_extend_values: [u32; 2],
        /// Per-side termination discriminators stored after the profile-normal slots.
        #[serde(default)]
        side_extent_discriminators: [u32; 2],
        /// Decoded extent form.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extent: Option<DesignExtrudeExtent>,
        /// Direction-reversal state.
        direction_reversed: bool,
        /// Whether the operation creates solid rather than sheet geometry.
        solid_operation: bool,
        /// Starting support.
        start: DesignExtrudeStart,
    },
}

impl TryFrom<DesignExtrudePrologueWire> for DesignExtrudePrologue {
    type Error = String;
    fn try_from(wire: DesignExtrudePrologueWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignExtrudePrologueWire::LegacyDistance {
                prefix_value,
                operation,
                extent_kind,
                direction_reversed,
                geometry_kind,
            } => {
                let prefix_zero = match prefix_value {
                    None => None,
                    Some(0) => Some(Sourced {
                        value: (),
                        offset: None,
                    }),
                    Some(_) => return Err("prefix_value must be zero".into()),
                };
                if extent_kind != 2 {
                    return Err("extent_kind must be 2 in a legacy distance prologue".into());
                }
                let solid_operation = match geometry_kind {
                    0 => false,
                    1 => true,
                    _ => return Err("geometry_kind must be 0 or 1".into()),
                };
                Self::LegacyDistance {
                    prefix_zero,
                    operation,
                    operation_offset: None,
                    extent_kind_offset: None,
                    direction_reversed,
                    direction_reversed_offset: None,
                    solid_operation,
                    solid_operation_offset: None,
                }
            }
            DesignExtrudePrologueWire::ReferenceAware {
                reference,
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                first_side_target_ordinal,
                extent,
                direction_reversed,
                solid_operation,
                start,
            } => Self::ReferenceAware {
                reference,
                operation,
                operation_offset: None,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets: None,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets: None,
                direction_reversed,
                direction_reversed_offset: None,
                solid_operation,
                solid_operation_offset: None,
                start,
                start_offset: None,
            },
            DesignExtrudePrologueWire::ShiftedReferenceAware {
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                extent,
                direction_reversed,
                solid_operation,
                start,
            } => Self::ShiftedReferenceAware {
                operation,
                operation_offset: None,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets: None,
                extent,
                direction_face_extend_offsets: None,
                direction_reversed,
                direction_reversed_offset: None,
                solid_operation,
                solid_operation_offset: None,
                start,
                start_offset: None,
            },
            DesignExtrudePrologueWire::LegacyShifted {
                operation_prefix_marker: marker,
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                extent,
                direction_reversed,
                solid_operation,
                start,
            } => Self::LegacyShifted {
                operation_prefix_marker: operation_prefix_marker(marker)?,
                operation,
                operation_offset: None,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets: None,
                extent,
                direction_face_extend_offsets: None,
                direction_reversed,
                direction_reversed_offset: None,
                solid_operation,
                solid_operation_offset: None,
                start,
                start_offset: None,
            },
        })
    }
}

impl From<DesignExtrudePrologue> for DesignExtrudePrologueWire {
    fn from(record: DesignExtrudePrologue) -> Self {
        match record {
            DesignExtrudePrologue::LegacyDistance {
                prefix_zero,
                operation,
                direction_reversed,
                solid_operation,
                ..
            } => Self::LegacyDistance {
                prefix_value: prefix_zero.map(|_| 0),
                operation,
                extent_kind: 2,
                direction_reversed,
                geometry_kind: u32::from(solid_operation),
            },
            DesignExtrudePrologue::ReferenceAware {
                reference,
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                first_side_target_ordinal,
                extent,
                direction_reversed,
                solid_operation,
                start,
                ..
            } => Self::ReferenceAware {
                reference,
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                first_side_target_ordinal,
                extent,
                direction_reversed,
                solid_operation,
                start,
            },
            DesignExtrudePrologue::ShiftedReferenceAware {
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                extent,
                direction_reversed,
                solid_operation,
                start,
                ..
            } => Self::ShiftedReferenceAware {
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                extent,
                direction_reversed,
                solid_operation,
                start,
            },
            DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker,
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                extent,
                direction_reversed,
                solid_operation,
                start,
                ..
            } => Self::LegacyShifted {
                operation_prefix_marker: operation_prefix_marker.map(|_| 1),
                operation,
                direction_face_extend_values,
                side_extent_discriminators,
                extent,
                direction_reversed,
                solid_operation,
                start,
            },
        }
    }
}

impl SourceOffsets for DesignExtrudePrologueReference {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset("record_index_offset", &mut self.record_index_offset);
        if let Some(marker) = &mut self.operation_prefix_marker {
            offsets.sourced("operation_prefix_marker_offset", marker);
        }
    }
}

impl SourceOffsets for DesignExtrudePrologue {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        match self {
            Self::LegacyDistance {
                prefix_zero,
                operation_offset,
                extent_kind_offset,
                direction_reversed_offset,
                solid_operation_offset,
                ..
            } => {
                if let Some(prefix) = prefix_zero {
                    offsets.sourced("prefix_value_offset", prefix);
                }
                offsets.offset("operation_offset", operation_offset);
                offsets.offset("extent_kind_offset", extent_kind_offset);
                offsets.offset("direction_reversed_offset", direction_reversed_offset);
                offsets.offset("geometry_kind_offset", solid_operation_offset);
            }
            Self::ReferenceAware {
                reference,
                operation_offset,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            } => {
                offsets.optional("reference", reference);
                offsets.offset("operation_offset", operation_offset);
                offsets.offsets(
                    "side_extent_discriminator_offsets",
                    side_extent_discriminator_offsets,
                );
                if let Some(ordinal) = first_side_target_ordinal {
                    offsets.offset(
                        "first_side_target_ordinal.scope_reference_ordinal_offset",
                        &mut ordinal.scope_reference_ordinal_offset,
                    );
                }
                offsets.offsets(
                    "direction_face_extend_offsets",
                    direction_face_extend_offsets,
                );
                offsets.offset("direction_reversed_offset", direction_reversed_offset);
                offsets.offset("solid_operation_offset", solid_operation_offset);
                offsets.offset("start_offset", start_offset);
            }
            Self::ShiftedReferenceAware {
                operation_offset,
                side_extent_discriminator_offsets,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            } => {
                offsets.offset("operation_offset", operation_offset);
                offsets.offsets(
                    "side_extent_discriminator_offsets",
                    side_extent_discriminator_offsets,
                );
                offsets.offsets(
                    "direction_face_extend_offsets",
                    direction_face_extend_offsets,
                );
                offsets.offset("direction_reversed_offset", direction_reversed_offset);
                offsets.offset("solid_operation_offset", solid_operation_offset);
                offsets.offset("start_offset", start_offset);
            }
            Self::LegacyShifted {
                operation_prefix_marker,
                operation_offset,
                side_extent_discriminator_offsets,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            } => {
                if let Some(marker) = operation_prefix_marker {
                    offsets.sourced("operation_prefix_marker_offset", marker);
                }
                offsets.offset("operation_offset", operation_offset);
                offsets.offsets(
                    "side_extent_discriminator_offsets",
                    side_extent_discriminator_offsets,
                );
                offsets.offsets(
                    "direction_face_extend_offsets",
                    direction_face_extend_offsets,
                );
                offsets.offset("direction_reversed_offset", direction_reversed_offset);
                offsets.offset("solid_operation_offset", solid_operation_offset);
                offsets.offset("start_offset", start_offset);
            }
        }
    }
}

impl DesignExtrudePrologue {
    /// Boolean result operation.
    pub(crate) fn operation(self) -> DesignExtrudeOperation {
        match self {
            Self::LegacyDistance { operation, .. }
            | Self::ReferenceAware { operation, .. }
            | Self::ShiftedReferenceAware { operation, .. }
            | Self::LegacyShifted { operation, .. } => operation,
        }
    }

    /// Decoded extent form.
    pub(crate) fn extent(self) -> Option<DesignExtrudeExtent> {
        match self {
            Self::LegacyDistance { .. } => Some(DesignExtrudeExtent::OneSidedDistance),
            Self::ReferenceAware { extent, .. } => Some(extent),
            Self::ShiftedReferenceAware { extent, .. } => Some(extent),
            Self::LegacyShifted { extent, .. } => extent,
        }
    }

    /// Direction-reversal state.
    pub(crate) fn direction_reversed(self) -> bool {
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
    pub(crate) fn solid_operation(self) -> bool {
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
    pub(crate) fn start(self) -> DesignExtrudeStart {
        match self {
            Self::LegacyDistance { .. } => DesignExtrudeStart::ProfilePlane,
            Self::ReferenceAware { start, .. }
            | Self::ShiftedReferenceAware { start, .. }
            | Self::LegacyShifted { start, .. } => start,
        }
    }
}

mod identity_rewrite;
