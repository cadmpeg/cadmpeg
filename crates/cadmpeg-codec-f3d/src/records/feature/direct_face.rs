// SPDX-License-Identifier: Apache-2.0
//! Direct face operations: offset faces, shell, thicken, move and draft.

use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::scalar::{Angle, FiniteReal};
use serde::{Deserialize, Serialize};
cadmpeg_core::named_optional_field!(deserialize_draft_flags, DesignDraftFlags, "flags");
cadmpeg_core::named_optional_field!(
    deserialize_transform_reference_offset,
    u64,
    "transform_reference_offset"
);
cadmpeg_core::named_optional_field!(deserialize_opposite_angle, Angle, "opposite_angle");
cadmpeg_core::named_optional_field!(deserialize_two_sided, DesignDraftTwoSided, "two_sided");

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
    /// Byte offset of the scalar.
    pub(crate) distance_offset: u64,
}

cadmpeg_core::named_optional_field!(deserialize_wall_slots, DesignShellWallSlots, "wall_slots");
cadmpeg_core::named_optional_field!(deserialize_outside_wall, DesignShellOutsideWall, "outside");
cadmpeg_core::named_optional_field!(
    deserialize_prefix_flag,
    DesignShellPrefixFlag,
    "prefix_flag"
);

/// Exact `Shell` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellOperation {
    /// Wall thickness in source centimetres.
    pub(crate) thickness: cadmpeg_ir::scalar::PositiveReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
    /// Whether the wall grows outward from the original boundary.
    pub(crate) outward: bool,
    /// Byte offset of the first present wall slot.
    pub(crate) outward_offset: u64,
    /// Fields that only the wall-slot form stores.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_wall_slots"
    )]
    pub(crate) wall_slots: Option<DesignShellWallSlots>,
    /// Offset-21 Boolean of the compact, marked, and counted prefixes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_prefix_flag"
    )]
    pub(crate) prefix_flag: Option<DesignShellPrefixFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) auxiliary_references: Vec<u32>,
}

/// Offset-21 Boolean of a compact, marked, or counted `Shell` prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignShellPrefixFlag {
    /// Stored Boolean.
    pub(crate) value: bool,
    /// Byte offset of the Boolean.
    pub(crate) offset: u64,
}

/// Fields of a `Shell` scope that stores its walls in optional slots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellWallSlots {
    /// Whether the selected faces extend along tangent faces.
    pub(crate) tangent_chain: bool,
    /// Whether offset corners are rounded instead of sharp.
    pub(crate) rounded_corners: bool,
    /// Outside wall of a scope that stores both walls.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outside_wall"
    )]
    pub(crate) outside: Option<DesignShellOutsideWall>,
}

/// Outside wall of a `Shell` scope that stores both walls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellOutsideWall {
    /// Wall thickness in source centimetres.
    pub(crate) thickness: cadmpeg_ir::scalar::PositiveReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
}

/// Exact `Thicken` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignThickenOperation {
    /// Signed thickness in source centimetres.
    pub(crate) signed_thickness: FiniteReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thicken_boolean"
    )]
    pub(crate) boolean: Option<DesignThickenBoolean>,
}

cadmpeg_core::named_optional_field!(deserialize_thicken_boolean, DesignThickenBoolean, "boolean");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignThickenBoolean {
    pub(crate) operation: DesignThickenBooleanOperation,
    pub(crate) operation_offset: u64,
    pub(crate) target_group_record_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignThickenBooleanOperation {
    Join,
    Cut,
    Intersect,
}

cadmpeg_core::named_optional_field!(deserialize_move_prefix, DesignMovePrefix, "prefix");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignMovePrefix {
    pub(crate) references: [DesignMoveReference; 4],
    pub(crate) codes: [u32; 2],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) matrix_omitted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum DesignMoveReference {
    Null,
    Local {
        record_index: u32,
    },
    Typed {
        record_index: u32,
        type_guid: String,
    },
}

impl DesignMovePrefix {
    pub(crate) fn form_offset(&self) -> Option<usize> {
        self.references.iter().try_fold(
            crate::layout::move_transform_frame_293::FIRST_REFERENCE + 8,
            |offset, reference| {
                offset.checked_add(match reference {
                    DesignMoveReference::Null => 1,
                    DesignMoveReference::Local { .. } => 11,
                    DesignMoveReference::Typed { type_guid, .. } => {
                        if !crate::bytes::is_guid_hyphenated(type_guid) {
                            return None;
                        }
                        crate::bytes::TYPED_LOCAL_REFERENCE_LEN
                    }
                })
            },
        )
    }

    pub(crate) fn full_references(&self) -> bool {
        self.references
            .iter()
            .all(|reference| !matches!(reference, DesignMoveReference::Null))
    }
}

/// Exact rigid transform carried by a Move feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignMoveOperation {
    /// Row-major model-space rigid transform in source centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the optional matrix payload.
    pub(crate) transform_offset: u64,
    /// Indexed record carrying `transform`.
    pub(crate) transform_record_index: u32,
    /// Source transform-form discriminator.
    pub(crate) form: DesignMoveForm,
    /// Byte offset of `form`.
    pub(crate) form_offset: u64,
    /// Byte offset of the scope's marked reference that selects the transform
    /// record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_reference_offset"
    )]
    pub(crate) transform_reference_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_move_prefix"
    )]
    pub(crate) prefix: Option<DesignMovePrefix>,
}

impl DesignMoveOperation {
    pub(crate) fn relative_form_offset(&self) -> Option<usize> {
        self.prefix.as_ref().map_or_else(
            || {
                Some(if self.form == DesignMoveForm::Form2 {
                    crate::layout::move_transform_frame_293::FORM
                } else {
                    crate::layout::move_transform_frame_253::FORM
                })
            },
            DesignMovePrefix::form_offset,
        )
    }

    pub(crate) fn preserves_orientation(&self) -> bool {
        use cadmpeg_ir::math::Vector3;
        let column = |index: usize| {
            Vector3::new(
                self.transform[0][index],
                self.transform[1][index],
                self.transform[2][index],
            )
        };
        column(0).cross(column(1)).dot(column(2)) > 0.0
    }
}

/// Source Move transform-form code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum DesignMoveForm {
    Form0,
    /// Source form 1.
    Form1,
    Form2,
    /// Source form 5.
    Form5,
    Form6,
}

impl TryFrom<u32> for DesignMoveForm {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Form0),
            1 => Ok(Self::Form1),
            2 => Ok(Self::Form2),
            5 => Ok(Self::Form5),
            6 => Ok(Self::Form6),
            _ => Err(format!("form must be 0, 1, 2, 5 or 6, not {value}")),
        }
    }
}

impl From<DesignMoveForm> for u32 {
    fn from(form: DesignMoveForm) -> Self {
        match form {
            DesignMoveForm::Form0 => 0,
            DesignMoveForm::Form1 => 1,
            DesignMoveForm::Form2 => 2,
            DesignMoveForm::Form5 => 5,
            DesignMoveForm::Form6 => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignModifyOperation {
    pub(crate) motion: DesignMoveOperation,
    pub(crate) body_group_record_indices: Vec<u32>,
    pub(crate) body_count_offset: u64,
}

/// Exact signed-angle lanes carried by a `Draft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignDraftOperation {
    /// Signed draft angle in radians.
    pub(crate) angle: Angle,
    /// Referenced draft-angle scalar record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the draft-angle scalar.
    pub(crate) angle_offset: u64,
    /// Opposite-side angle scalar record.
    pub(crate) opposite_angle_record_index: u32,
    /// Byte offset of the opposite-side angle scalar.
    pub(crate) opposite_angle_offset: u64,
    /// Opposite-side angle of a two-sided draft, in radians.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle"
    )]
    pub(crate) opposite_angle: Option<Angle>,
    /// Flags of a scope whose prologue names both angle lanes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft_flags"
    )]
    pub(crate) flags: Option<DesignDraftFlags>,
}

/// Flags that the `Draft` prologue stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignDraftFlags {
    /// Whether the pull direction is opposite the neutral plane normal.
    pub(crate) flipped: bool,
    /// Whether the selected faces extend along tangent faces.
    pub(crate) tangent_chain: bool,
    /// Two-sided mode; absent in the single-angle mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_two_sided"
    )]
    pub(crate) two_sided: Option<DesignDraftTwoSided>,
}

/// Source mode code of a two-sided `Draft`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum DesignDraftTwoSided {
    /// Mode 2: the second side has its own angle.
    TwoAngles,
    /// Mode 3: the second side has the first angle.
    Symmetric,
}

impl TryFrom<u32> for DesignDraftTwoSided {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            2 => Ok(Self::TwoAngles),
            3 => Ok(Self::Symmetric),
            _ => Err(format!("two-sided Draft mode must be 2 or 3, not {value}")),
        }
    }
}

impl From<DesignDraftTwoSided> for u32 {
    fn from(mode: DesignDraftTwoSided) -> Self {
        match mode {
            DesignDraftTwoSided::TwoAngles => 2,
            DesignDraftTwoSided::Symmetric => 3,
        }
    }
}

mod identity_rewrite;

#[cfg(test)]
mod tests {
    use super::{
        DesignShellOperation, DesignShellOutsideWall, DesignShellPrefixFlag, DesignShellWallSlots,
    };

    fn operation(outside: Option<DesignShellOutsideWall>) -> DesignShellOperation {
        DesignShellOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.1).unwrap(),
            thickness_record_index: 291,
            thickness_offset: 100,
            outward: false,
            outward_offset: 23,
            wall_slots: Some(DesignShellWallSlots {
                tangent_chain: true,
                rounded_corners: false,
                outside,
            }),
            prefix_flag: None,
            auxiliary_references: Vec::new(),
        }
    }

    #[test]
    fn wall_slots_are_present_or_their_keys_are_absent() {
        let both = operation(Some(DesignShellOutsideWall {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.3).unwrap(),
            thickness_record_index: 294,
            thickness_offset: 200,
        }));
        let one = operation(None);
        let without = DesignShellOperation {
            wall_slots: None,
            ..one.clone()
        };
        let flagged = DesignShellOperation {
            prefix_flag: Some(DesignShellPrefixFlag {
                value: true,
                offset: 21,
            }),
            ..without.clone()
        };
        for value in [both, one, without, flagged] {
            let wire = serde_json::to_value(&value).unwrap();
            assert_eq!(
                serde_json::from_value::<DesignShellOperation>(wire).unwrap(),
                value
            );
        }
        let mut wire = serde_json::to_value(operation(None)).unwrap();
        assert!(wire["wall_slots"].get("outside").is_none());
        wire["wall_slots"]["outside"] = serde_json::Value::Null;
        let error = serde_json::from_value::<DesignShellOperation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside"), "{error}");

        let mut wire = serde_json::to_value(operation(None)).unwrap();
        wire["wall_slots"] = serde_json::Value::Null;
        let error = serde_json::from_value::<DesignShellOperation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("wall_slots"), "{error}");
    }
}
