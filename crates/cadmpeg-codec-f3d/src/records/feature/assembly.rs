// SPDX-License-Identifier: Apache-2.0

use super::{
    ConstructionRecipeKind, DesignClassTag, DesignExternalVersion, DesignHoleConstruction,
    DesignRecipeReference, DesignRelaxedGuidText, DesignWorkPointConstruction, Located,
    SketchPlacementMatrix,
};
use serde::{Deserialize, Serialize};

/// Domain of the two scalar limits carried by a legacy As-built scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignAssemblyLimitKind {
    /// Limits on the joint's angular degree of freedom.
    #[default]
    Angular,
    /// Limits on the joint's linear degree of freedom.
    Linear,
}

/// Ordered lower and upper limits carried by a legacy As-built assembly scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignAssemblyLimitsWire",
    into = "DesignAssemblyLimitsWire"
)]
pub struct DesignAssemblyLimits {
    /// Degree-of-freedom domain of the limits.
    #[serde(default)]
    pub kind: DesignAssemblyLimitKind,
    /// Lower bound in the domain's native units.
    minimum: f64,
    /// Upper bound in the domain's native units.
    maximum: f64,
    /// Parameter-owner records for the lower and upper bounds.
    pub owner_record_indices: [u32; 2],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: [u64; 2],
}

/// Wire fields for finite ordered assembly limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignAssemblyLimitsWire {
    /// Degree-of-freedom domain of the limits.
    #[serde(default)]
    pub kind: DesignAssemblyLimitKind,
    /// Lower bound in the domain's native units.
    pub minimum: f64,
    /// Upper bound in the domain's native units.
    pub maximum: f64,
    /// Parameter-owner records for the lower and upper bounds.
    pub owner_record_indices: [u32; 2],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub value_offsets: [u64; 2],
}

impl DesignAssemblyLimits {
    pub(crate) fn minimum(&self) -> f64 {
        self.minimum
    }
    pub(crate) fn maximum(&self) -> f64 {
        self.maximum
    }
}

impl TryFrom<DesignAssemblyLimitsWire> for DesignAssemblyLimits {
    type Error = String;
    fn try_from(wire: DesignAssemblyLimitsWire) -> Result<Self, Self::Error> {
        if !wire.minimum.is_finite() || !wire.maximum.is_finite() || wire.minimum > wire.maximum {
            return Err("assembly limits minimum and maximum must be finite and ordered".into());
        }
        Ok(Self {
            kind: wire.kind,
            minimum: wire.minimum,
            maximum: wire.maximum,
            owner_record_indices: wire.owner_record_indices,
            value_offsets: wire.value_offsets,
        })
    }
}
impl From<DesignAssemblyLimits> for DesignAssemblyLimitsWire {
    fn from(value: DesignAssemblyLimits) -> Self {
        Self {
            kind: value.kind,
            minimum: value.minimum,
            maximum: value.maximum,
            owner_record_indices: value.owner_record_indices,
            value_offsets: value.value_offsets,
        }
    }
}

/// Exact solved frame carried by a legacy 421-byte `As-built` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignAssemblySolvedFrame {
    /// Frame-carrier record named by reference-table entry eight.
    pub reference_record_index: u32,
    /// Byte offset of the frame-carrier reference in the scope.
    pub reference_offset: u64,
    /// Byte offset of the frame-carrier indexed header.
    pub record_byte_offset: u64,
    /// Dynamic class of the frame-carrier indexed record.
    pub class_tag: DesignClassTag,
    /// Row-major solved connector frame.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
}

/// A construction and its face selection in a legacy assembly operand.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignAssemblyLegacyOperand<C> {
    pub construction_class_tag: DesignClassTag,
    pub construction: C,
    pub selection: DesignAssemblyLegacySelection,
    pub reference_offset: u64,
}

/// The ordered point and hole constructions of a legacy 421-byte assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignAssemblyLegacyOperands {
    point: DesignAssemblyLegacyOperand<Box<DesignWorkPointConstruction>>,
    hole: DesignAssemblyLegacyOperand<Box<DesignHoleConstruction>>,
}

impl DesignAssemblyLegacyOperands {
    /// Admit legacy operand carriers with finite solved positions.
    pub fn try_new(
        point: DesignAssemblyLegacyOperand<Box<DesignWorkPointConstruction>>,
        hole: DesignAssemblyLegacyOperand<Box<DesignHoleConstruction>>,
    ) -> Result<Self, String> {
        if !point
            .construction
            .position
            .iter()
            .chain(hole.construction.position.iter())
            .all(|value| value.is_finite())
        {
            return Err("legacy_operand_carriers position must be finite".into());
        }
        Ok(Self { point, hole })
    }

    pub(crate) fn references(&self) -> [Located<u32>; 2] {
        [
            Located {
                value: self.point.construction.point_record_index,
                offset: self.point.reference_offset,
            },
            Located {
                value: self.hole.construction.point_record_index,
                offset: self.hole.reference_offset,
            },
        ]
    }

    pub(crate) fn selections(&self) -> [&DesignAssemblyLegacySelection; 2] {
        [&self.point.selection, &self.hole.selection]
    }

    pub(crate) fn frames(
        &self,
        solved: &DesignAssemblySolvedFrame,
    ) -> Result<[DesignAssemblyOperandFrame; 2], String> {
        let references = self.references();
        let positions = [
            self.point.construction.position,
            self.hole.construction.position,
        ];
        let frames: [Result<DesignAssemblyOperandFrame, String>; 2] = [0, 1].map(|index| {
            let mut transform = solved.transform.rows();
            for (row, value) in positions[index].into_iter().enumerate() {
                transform[row][3] = value;
            }
            Ok(DesignAssemblyOperandFrame {
                reference_record_index: references[index].value,
                reference_offset: references[index].offset,
                transform: transform.try_into()?,
                transform_offset: solved.transform_offset,
            })
        });
        let [first, second] = frames;
        Ok([first?, second?])
    }

    fn from_wire(
        wire: [DesignAssemblyLegacyOperandWire; 2],
        solved: &DesignAssemblySolvedFrame,
    ) -> Result<Self, String> {
        let [point, hole] = wire;
        let DesignAssemblyLegacyConstruction::Point(point_construction) = point.construction else {
            return Err("legacy_operand_carriers[0].construction requires a point".into());
        };
        let DesignAssemblyLegacyConstruction::Hole(hole_construction) = hole.construction else {
            return Err("legacy_operand_carriers[1].construction requires a hole".into());
        };
        if point.construction_record_index != point_construction.point_record_index
            || hole.construction_record_index != hole_construction.point_record_index
        {
            return Err(
                "legacy_operand_carriers construction_record_index disagrees with construction"
                    .into(),
            );
        }
        if point.construction_byte_offset != point_construction.point_record_byte_offset
            || hole.construction_byte_offset != hole_construction.point_record_byte_offset
        {
            return Err(
                "legacy_operand_carriers construction_byte_offset disagrees with construction"
                    .into(),
            );
        }
        let carriers = Self::try_new(
            DesignAssemblyLegacyOperand {
                construction_class_tag: point
                    .construction_class_tag
                    .try_into()
                    .map_err(|error| format!("construction_class_tag: {error}"))?,
                construction: point_construction,
                selection: point.selection,
                reference_offset: point.frame.reference_offset,
            },
            DesignAssemblyLegacyOperand {
                construction_class_tag: hole
                    .construction_class_tag
                    .try_into()
                    .map_err(|error| format!("construction_class_tag: {error}"))?,
                construction: hole_construction,
                selection: hole.selection,
                reference_offset: hole.frame.reference_offset,
            },
        )?;
        if [point.frame, hole.frame] != carriers.frames(solved)? {
            return Err(
                "legacy_operand_carriers frame disagrees with construction and solved_frame".into(),
            );
        }
        Ok(carriers)
    }

    fn into_wire(
        self,
        solved: &DesignAssemblySolvedFrame,
    ) -> Result<[DesignAssemblyLegacyOperandWire; 2], String> {
        let [point_frame, hole_frame] = self.frames(solved)?;
        Ok([
            DesignAssemblyLegacyOperandWire {
                construction_record_index: self.point.construction.point_record_index,
                construction_byte_offset: self.point.construction.point_record_byte_offset,
                construction_class_tag: self.point.construction_class_tag.into(),
                construction: DesignAssemblyLegacyConstruction::Point(self.point.construction),
                selection: self.point.selection,
                frame: point_frame,
            },
            DesignAssemblyLegacyOperandWire {
                construction_record_index: self.hole.construction.point_record_index,
                construction_byte_offset: self.hole.construction.point_record_byte_offset,
                construction_class_tag: self.hole.construction_class_tag.into(),
                construction: DesignAssemblyLegacyConstruction::Hole(self.hole.construction),
                selection: self.hole.selection,
                frame: hole_frame,
            },
        ])
    }
}

/// Exact construction and face-selection pair carried by a legacy 421-byte
/// `As-built` operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAssemblyLegacyOperandWire {
    /// Primary construction record named by the scope reference table.
    construction_record_index: u32,
    /// Byte offset of the primary construction record header.
    construction_byte_offset: u64,
    /// Dynamic class of the primary construction record.
    construction_class_tag: String,
    /// Exact point or point-and-direction construction.
    construction: DesignAssemblyLegacyConstruction,
    /// Face-selection record paired with the construction record.
    selection: DesignAssemblyLegacySelection,
    /// Connector-local frame derived from the stored solved-frame orientation
    /// and this operand's construction position.
    frame: DesignAssemblyOperandFrame,
}

/// Construction carrier family used by a legacy 421-byte `As-built` operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum DesignAssemblyLegacyConstruction {
    /// Point-only connector construction.
    Point(Box<DesignWorkPointConstruction>),
    /// Point-and-direction connector construction.
    Hole(Box<DesignHoleConstruction>),
}

/// Exact face-recipe selection paired with a legacy 421-byte construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignAssemblyLegacySelection {
    /// Indexed selection record.
    pub record_index: u32,
    /// Byte offset of the selection record header.
    pub byte_offset: u64,
    /// Dynamic class of the selection record.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE payload.
    pub asset_id_offset: u64,
    /// Context UUID qualifying the selection.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE payload.
    pub context_id_offset: u64,
    /// Indexed record containing the face recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Construction-recipe arena id.
    pub recipe_id: String,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Persistent selector/reference tails carried by the recipe prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the indexed record immediately after the recipe.
    pub next_byte_offset: u64,
}

/// Alignment scalars carried by an assembly-operation scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignAssemblyAlignmentSerde")]
pub struct DesignAssemblyAlignment {
    /// Signed alignment rotation in radians.
    angle: f64,
    /// Signed local-frame translation in source centimetres.
    offset: [f64; 3],
    /// Parameter-owner records and their evaluated-value locations.
    pub owners: Vec<Located<u32>>,
    /// Datum, legacy solved-carrier, or qualified-operand form.
    pub form: Option<DesignAssemblyAlignmentForm>,
}

/// One assembly operand frame and its source qualifier.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignQualifiedAssemblyOperand {
    pub frame: DesignAssemblyOperandFrame,
    pub qualifier: DesignAssemblyOperandQualifier,
}

/// Native evidence retained by an assembly-alignment form.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignAssemblyAlignmentForm {
    DatumEnvelope {
        joint_origin_scope_record_index: u32,
    },
    LimitsOnly {
        limits: DesignAssemblyLimits,
    },
    SolvedOnly {
        solved_frame: DesignAssemblySolvedFrame,
        limits: Option<DesignAssemblyLimits>,
    },
    LegacyAsBuilt421 {
        carriers: DesignAssemblyLegacyOperands,
        solved_frame: DesignAssemblySolvedFrame,
        limits: Option<DesignAssemblyLimits>,
        /// Preserve omission of the redundant frame field in older native records.
        frames_field_present: bool,
    },
    Frames {
        frames: [DesignAssemblyOperandFrame; 2],
    },
    Qualified([DesignQualifiedAssemblyOperand; 2]),
}

impl DesignAssemblyAlignmentForm {
    pub(crate) fn qualified(
        frames: [DesignAssemblyOperandFrame; 2],
        qualifiers: [DesignAssemblyOperandQualifier; 2],
    ) -> Self {
        let [first_frame, second_frame] = frames;
        let [first_qualifier, second_qualifier] = qualifiers;
        Self::Qualified([
            DesignQualifiedAssemblyOperand {
                frame: first_frame,
                qualifier: first_qualifier,
            },
            DesignQualifiedAssemblyOperand {
                frame: second_frame,
                qualifier: second_qualifier,
            },
        ])
    }
}

impl DesignAssemblyAlignment {
    pub(crate) fn try_new(
        angle: f64,
        offset: [f64; 3],
        owners: Vec<Located<u32>>,
        form: Option<DesignAssemblyAlignmentForm>,
    ) -> Result<Self, String> {
        if !angle.is_finite() {
            return Err("assembly alignment angle must be finite".into());
        }
        if !offset.iter().all(|value| value.is_finite()) {
            return Err("assembly alignment offset must be finite".into());
        }
        Ok(Self {
            angle,
            offset,
            owners,
            form,
        })
    }
    pub(crate) fn angle(&self) -> f64 {
        self.angle
    }
    pub(crate) fn offset(&self) -> [f64; 3] {
        self.offset
    }

    pub(crate) fn operand_frames(&self) -> Option<[DesignAssemblyOperandFrame; 2]> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::Frames { frames } => Some(frames.clone()),
            DesignAssemblyAlignmentForm::Qualified(operands) => {
                Some(operands.each_ref().map(|operand| operand.frame.clone()))
            }
            DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                ..
            } => carriers.frames(solved_frame).ok(),
            DesignAssemblyAlignmentForm::DatumEnvelope { .. }
            | DesignAssemblyAlignmentForm::LimitsOnly { .. }
            | DesignAssemblyAlignmentForm::SolvedOnly { .. } => None,
        }
    }

    pub(crate) fn solved_frame(&self) -> Option<&DesignAssemblySolvedFrame> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, .. }
            | DesignAssemblyAlignmentForm::LegacyAsBuilt421 { solved_frame, .. } => {
                Some(solved_frame)
            }
            _ => None,
        }
    }

    pub(crate) fn operand_qualifiers(&self) -> Option<[DesignAssemblyOperandQualifier; 2]> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::Qualified(operands) => {
                Some(operands.each_ref().map(|operand| operand.qualifier.clone()))
            }
            _ => None,
        }
    }

    /// Return both occurrence paths when every operand uses that qualifier form.
    pub(crate) fn operand_paths(&self) -> Option<[DesignAssemblyOperandPath; 2]> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::Qualified(operands) => {
                let [Some(first), Some(second)] = operands
                    .each_ref()
                    .map(|operand| operand.qualifier.occurrence_path())
                else {
                    return None;
                };
                Some([first.clone(), second.clone()])
            }
            _ => None,
        }
    }

    pub(crate) fn limits(&self) -> Option<&DesignAssemblyLimits> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::LimitsOnly { limits } => Some(limits),
            DesignAssemblyAlignmentForm::SolvedOnly { limits, .. }
            | DesignAssemblyAlignmentForm::LegacyAsBuilt421 { limits, .. } => limits.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn joint_origin_scope_record_index(&self) -> Option<u32> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index,
            } => Some(*joint_origin_scope_record_index),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAssemblyAlignmentSerde {
    angle: f64,
    offset: [f64; 3],
    owner_record_indices: Vec<u32>,
    value_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operand_frames: Option<[DesignAssemblyOperandFrame; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_operand_carriers: Option<[DesignAssemblyLegacyOperandWire; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    solved_frame: Option<DesignAssemblySolvedFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operand_qualifiers: Option<[DesignAssemblyOperandQualifier; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "angular_limits")]
    limits: Option<DesignAssemblyLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    joint_origin_scope_record_index: Option<u32>,
}

impl TryFrom<DesignAssemblyAlignmentSerde> for DesignAssemblyAlignment {
    type Error = String;

    fn try_from(wire: DesignAssemblyAlignmentSerde) -> Result<Self, Self::Error> {
        if wire.owner_record_indices.len() != wire.value_offsets.len() {
            return Err("owner_record_indices and value_offsets must have equal lengths".into());
        }
        let owners = wire
            .owner_record_indices
            .into_iter()
            .zip(wire.value_offsets)
            .map(|(value, offset)| Located { value, offset })
            .collect();
        let form = match (
            wire.legacy_operand_carriers,
            wire.solved_frame,
            wire.operand_frames,
            wire.operand_qualifiers,
            wire.limits,
            wire.joint_origin_scope_record_index,
        ) {
            (None, None, None, None, None, Some(joint_origin_scope_record_index)) => Some(
                DesignAssemblyAlignmentForm::DatumEnvelope { joint_origin_scope_record_index }
            ),
            (Some(carriers), Some(solved_frame), frames, None, limits, None) => {
                let carriers = DesignAssemblyLegacyOperands::from_wire(carriers, &solved_frame)?;
                let derived_frames = carriers.frames(&solved_frame)?;
                if frames.as_ref().is_some_and(|frames| frames != &derived_frames) {
                    return Err("operand_frames must match legacy_operand_carriers frames".into());
                }
                Some(DesignAssemblyAlignmentForm::LegacyAsBuilt421 { carriers, solved_frame, limits, frames_field_present: frames.is_some() })
            }
            (None, Some(solved_frame), None, None, limits, None) => Some(
                DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, limits }
            ),
            (None, None, None, None, Some(limits), None) => Some(
                DesignAssemblyAlignmentForm::LimitsOnly { limits }
            ),
            (None, None, Some(frames), None, None, None) => Some(
                DesignAssemblyAlignmentForm::Frames { frames }
            ),
            (None, None, Some(frames), Some(qualifiers), None, None) => Some(
                DesignAssemblyAlignmentForm::qualified(frames, qualifiers)
            ),
            (None, None, None, None, None, None) => None,
            _ => return Err("assembly alignment operand_frames, legacy_operand_carriers, solved_frame, operand_qualifiers, limits, and joint_origin_scope_record_index disagree with one form".into()),
        };
        Self::try_new(wire.angle, wire.offset, owners, form)
    }
}

impl Serialize for DesignAssemblyAlignment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignAssemblyAlignmentSerde::try_from(self.clone())
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl TryFrom<DesignAssemblyAlignment> for DesignAssemblyAlignmentSerde {
    type Error = String;
    fn try_from(alignment: DesignAssemblyAlignment) -> Result<Self, Self::Error> {
        let (
            operand_frames,
            legacy_operand_carriers,
            solved_frame,
            operand_qualifiers,
            limits,
            joint_origin_scope_record_index,
        ) = match alignment.form {
            None => (None, None, None, None, None, None),
            Some(DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index,
            }) => (
                None,
                None,
                None,
                None,
                None,
                Some(joint_origin_scope_record_index),
            ),
            Some(DesignAssemblyAlignmentForm::LimitsOnly { limits }) => {
                (None, None, None, None, Some(limits), None)
            }
            Some(DesignAssemblyAlignmentForm::SolvedOnly {
                solved_frame,
                limits,
            }) => (None, None, Some(solved_frame), None, limits, None),
            Some(DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                limits,
                frames_field_present,
            }) => {
                let frames = frames_field_present
                    .then(|| carriers.frames(&solved_frame))
                    .transpose()?;
                let carriers = carriers.into_wire(&solved_frame)?;
                (
                    frames,
                    Some(carriers),
                    Some(solved_frame),
                    None,
                    limits,
                    None,
                )
            }
            Some(DesignAssemblyAlignmentForm::Frames { frames }) => {
                (Some(frames), None, None, None, None, None)
            }
            Some(DesignAssemblyAlignmentForm::Qualified(
                [DesignQualifiedAssemblyOperand {
                    frame: first_frame,
                    qualifier: first_qualifier,
                }, DesignQualifiedAssemblyOperand {
                    frame: second_frame,
                    qualifier: second_qualifier,
                }],
            )) => (
                Some([first_frame, second_frame]),
                None,
                None,
                Some([first_qualifier, second_qualifier]),
                None,
                None,
            ),
        };
        let (owner_record_indices, value_offsets) = alignment
            .owners
            .into_iter()
            .map(|owner| (owner.value, owner.offset))
            .unzip();
        Ok(Self {
            angle: alignment.angle,
            offset: alignment.offset,
            owner_record_indices,
            value_offsets,
            operand_frames,
            legacy_operand_carriers,
            solved_frame,
            operand_qualifiers,
            limits,
            joint_origin_scope_record_index,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignAssemblyAxialOperandTarget {
    /// Connector object selected inside a placed `Component Insert` occurrence.
    ComponentInsertOccurrence {
        /// `Component Insert` scope whose placement has the selected occurrence role.
        component_insert_scope_record_index: u32,
        /// Construction carrier referenced by the operand frame.
        construction_record_index: u32,
        /// Dynamic class of the construction carrier's primary record.
        construction_class_tag: DesignClassTag,
        /// Byte offset of the construction carrier's primary indexed header.
        construction_byte_offset: u64,
        /// Byte offset of the construction carrier's transform.
        construction_transform_offset: u64,
        /// Byte offsets of the two axis-record indices in the construction carrier.
        axis_record_index_offsets: [u64; 2],
        /// Dynamic class of the construction carrier's paired record.
        construction_paired_class_tag: DesignClassTag,
        /// Byte offset of the construction carrier's paired indexed header.
        construction_paired_byte_offset: u64,
        /// Two axis selectors that identify the same connector object.
        selectors: Box<[DesignAssemblyAxialSelectorIdentity; 2]>,
    },
    /// Datum connector owned directly by the current document root.
    DocumentRootJointOrigin {
        /// Referenced `JointOrigin` feature scope.
        scope_record_index: u32,
    },
}

/// Persistent connector identity carried by one axial assembly selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignAssemblyAxialSelectorIdentityWire",
    into = "DesignAssemblyAxialSelectorIdentityWire"
)]
pub struct DesignAssemblyAxialSelectorIdentity {
    /// Axis record named by the operand construction carrier.
    pub axis_record_index: u32,
    /// Dynamic class of the axis record's primary indexed header.
    pub axis_class_tag: DesignClassTag,
    /// Byte offset of the axis record's primary indexed header.
    pub axis_byte_offset: u64,
    /// Dynamic class of the axis record's paired indexed header.
    pub axis_paired_class_tag: DesignClassTag,
    /// Byte offset of the axis record's paired indexed header.
    pub axis_paired_byte_offset: u64,
    /// Selector record three indices after the axis record.
    pub selector_record_index: u32,
    /// Dynamic class of the selector record's primary indexed header.
    pub selector_class_tag: DesignClassTag,
    /// Byte offset of the selector record's primary indexed header.
    pub selector_byte_offset: u64,
    /// Dynamic class of the selector record's paired indexed header.
    pub selector_paired_class_tag: DesignClassTag,
    /// Byte offset of the selector record's paired indexed header.
    pub selector_paired_byte_offset: u64,
    /// Nested record named by the selector prefix.
    pub nested_record_index: u32,
    /// Byte offset of `nested_record_index`.
    pub nested_record_index_offset: u64,
    /// Asset GUID of the enclosing selector.
    pub selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    pub selector_asset_id_offset: u64,
    /// Context GUID of the enclosing selector.
    pub selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    pub selector_context_id_offset: u64,
    /// Axis-specific same-segment occurrence reference.
    pub occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub occurrence_reference_offset: u64,
    /// Entity reference of the selected object in the referenced document.
    pub external_object_reference: u64,
    /// Byte offset of `external_object_reference`.
    pub external_object_reference_offset: u64,
    /// Segment carried by the cross-document object reference.
    pub external_segment: u32,
    /// Byte offset of `external_segment`.
    pub external_segment_offset: u64,
    /// Asset GUID carried by the cross-document object reference.
    pub external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    pub external_asset_id_offset: u64,
    /// Link name carried by the cross-document object reference.
    pub external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    pub external_version: Option<DesignExternalVersion>,
    /// Embedded record that carries the selected occurrence role.
    pub role_record_index: u32,
    /// Dynamic class of the occurrence-role record.
    pub role_class_tag: DesignClassTag,
    /// Byte offset of the occurrence-role record's indexed header.
    pub role_byte_offset: u64,
    /// Occurrence-role GUID joining this selector to a component insertion.
    pub occurrence_role: DesignRelaxedGuidText,
    /// Byte offset of `occurrence_role`.
    pub occurrence_role_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignAssemblyAxialSelectorIdentityWire {
    /// Axis record named by the operand construction carrier.
    axis_record_index: u32,
    /// Dynamic class of the axis record's primary indexed header.
    axis_class_tag: String,
    /// Byte offset of the axis record's primary indexed header.
    axis_byte_offset: u64,
    /// Dynamic class of the axis record's paired indexed header.
    axis_paired_class_tag: String,
    /// Byte offset of the axis record's paired indexed header.
    axis_paired_byte_offset: u64,
    /// Selector record three indices after the axis record.
    selector_record_index: u32,
    /// Dynamic class of the selector record's primary indexed header.
    selector_class_tag: String,
    /// Byte offset of the selector record's primary indexed header.
    selector_byte_offset: u64,
    /// Dynamic class of the selector record's paired indexed header.
    selector_paired_class_tag: String,
    /// Byte offset of the selector record's paired indexed header.
    selector_paired_byte_offset: u64,
    /// Nested record named by the selector prefix.
    nested_record_index: u32,
    /// Byte offset of `nested_record_index`.
    nested_record_index_offset: u64,
    /// Asset GUID of the enclosing selector.
    selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing selector.
    selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Axis-specific same-segment occurrence reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the selected object in the referenced document.
    external_object_reference: u64,
    /// Byte offset of `external_object_reference`.
    external_object_reference_offset: u64,
    /// Segment carried by the cross-document object reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Asset GUID carried by the cross-document object reference.
    external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document object reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_property_key: Option<DesignRelaxedGuidText>,
    /// Byte offset of `external_property_key` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_version_urn_offset: Option<u64>,
    /// Embedded record that carries the selected occurrence role.
    role_record_index: u32,
    /// Dynamic class of the occurrence-role record.
    role_class_tag: String,
    /// Byte offset of the occurrence-role record's indexed header.
    role_byte_offset: u64,
    /// Occurrence-role GUID joining this selector to a component insertion.
    occurrence_role: DesignRelaxedGuidText,
    /// Byte offset of `occurrence_role`.
    occurrence_role_offset: u64,
}

impl TryFrom<DesignAssemblyAxialSelectorIdentityWire> for DesignAssemblyAxialSelectorIdentity {
    type Error = String;
    fn try_from(wire: DesignAssemblyAxialSelectorIdentityWire) -> Result<Self, Self::Error> {
        Ok(Self {
            axis_record_index: wire.axis_record_index,
            axis_class_tag: wire.axis_class_tag.try_into().map_err(|error| format!("axis_class_tag: {error}"))?,
            axis_byte_offset: wire.axis_byte_offset,
            axis_paired_class_tag: wire.axis_paired_class_tag.try_into().map_err(|error| format!("axis_paired_class_tag: {error}"))?,
            axis_paired_byte_offset: wire.axis_paired_byte_offset,
            selector_record_index: wire.selector_record_index,
            selector_class_tag: wire.selector_class_tag.try_into().map_err(|error| format!("selector_class_tag: {error}"))?,
            selector_byte_offset: wire.selector_byte_offset,
            selector_paired_class_tag: wire.selector_paired_class_tag.try_into().map_err(|error| format!("selector_paired_class_tag: {error}"))?,
            selector_paired_byte_offset: wire.selector_paired_byte_offset,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_object_reference: wire.external_object_reference,
            external_object_reference_offset: wire.external_object_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id: wire.external_asset_id,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version: match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
                (None, None, None, None) => None,
                (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
                _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
            },
            role_record_index: wire.role_record_index,
            role_class_tag: wire.role_class_tag.try_into().map_err(|error| format!("role_class_tag: {error}"))?,
            role_byte_offset: wire.role_byte_offset,
            occurrence_role: wire.occurrence_role,
            occurrence_role_offset: wire.occurrence_role_offset,
        })
    }
}

impl From<DesignAssemblyAxialSelectorIdentity> for DesignAssemblyAxialSelectorIdentityWire {
    fn from(record: DesignAssemblyAxialSelectorIdentity) -> Self {
        Self {
            axis_record_index: record.axis_record_index,
            axis_class_tag: record.axis_class_tag.into(),
            axis_byte_offset: record.axis_byte_offset,
            axis_paired_class_tag: record.axis_paired_class_tag.into(),
            axis_paired_byte_offset: record.axis_paired_byte_offset,
            selector_record_index: record.selector_record_index,
            selector_class_tag: record.selector_class_tag.into(),
            selector_byte_offset: record.selector_byte_offset,
            selector_paired_class_tag: record.selector_paired_class_tag.into(),
            selector_paired_byte_offset: record.selector_paired_byte_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            selector_asset_id: record.selector_asset_id,
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_object_reference: record.external_object_reference,
            external_object_reference_offset: record.external_object_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.external_asset_id,
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
            role_record_index: record.role_record_index,
            role_class_tag: record.role_class_tag.into(),
            role_byte_offset: record.role_byte_offset,
            occurrence_role: record.occurrence_role,
            occurrence_role_offset: record.occurrence_role_offset,
        }
    }
}

impl DesignAssemblyAxialSelectorIdentity {
    /// Report whether two axis selectors carry the same persistent connector identity.
    pub(crate) fn selects_same_object(&self, other: &Self) -> bool {
        self.selector_asset_id
            .as_str()
            .eq_ignore_ascii_case(other.selector_asset_id.as_str())
            && self
                .selector_context_id
                .as_str()
                .eq_ignore_ascii_case(other.selector_context_id.as_str())
            && self.external_object_reference == other.external_object_reference
            && self.external_segment == other.external_segment
            && self
                .external_asset_id
                .as_str()
                .eq_ignore_ascii_case(other.external_asset_id.as_str())
            && self.external_link_name == other.external_link_name
            && match (&self.external_version, &other.external_version) {
                (None, None) => true,
                (Some(first), Some(second)) => {
                    first
                        .property_key
                        .value
                        .as_str()
                        .eq_ignore_ascii_case(second.property_key.value.as_str())
                        && first.version_urn.value == second.version_urn.value
                }
                _ => false,
            }
    }
}

/// Exact reference chain from an assembly scope to one occurrence-path record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignAssemblyOperandPathLink {
    /// Byte offset of the locator-record index in the assembly scope.
    pub locator_reference_offset: u64,
    /// Locator-record index named by the assembly scope.
    pub locator_record_index: u32,
    /// Dynamic indexed-record class carrying the locator.
    pub locator_class_tag: DesignClassTag,
    /// Byte offset of the locator's indexed header.
    pub locator_byte_offset: u64,
    /// Byte offset of the assembly-scope backlink in the locator.
    pub locator_scope_reference_offset: u64,
    /// Wrapper-record index named by the locator.
    pub wrapper_record_index: u32,
    /// Byte offset of the wrapper-record index in the locator.
    pub wrapper_reference_offset: u64,
    /// Dynamic indexed-record class carrying the wrapper.
    pub wrapper_class_tag: DesignClassTag,
    /// Byte offset of the wrapper's indexed header.
    pub wrapper_byte_offset: u64,
    /// Byte offset of the path-record index in the wrapper.
    pub path_reference_offset: u64,
}

/// Counted occurrence path qualifying one assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignAssemblyOperandPathWire",
    into = "DesignAssemblyOperandPathWire"
)]
pub struct DesignAssemblyOperandPath {
    link: DesignAssemblyOperandPathLink,
    pub record_index: u32,
    class_tag: DesignClassTag,
    byte_offset: u64,
    /// Ordered occurrence GUIDs and their UTF-16 code-unit locations.
    occurrence_guids: Vec<Located<DesignRelaxedGuidText>>,
    /// Ordered identity GUIDs and their UTF-16 code-unit locations.
    identity_guids: Vec<Located<DesignRelaxedGuidText>>,
}

/// Counted occurrence path qualifying one assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAssemblyOperandPathWire {
    /// Exact ordered scope-to-locator-to-wrapper reference chain.
    link: DesignAssemblyOperandPathLink,
    /// Path-record index.
    record_index: u32,
    /// Indexed-record class carrying this path.
    class_tag: String,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Ordered occurrence GUIDs from the outermost occurrence to the selected occurrence.
    occurrence_guids: Vec<DesignRelaxedGuidText>,
    /// Byte offsets of the UTF-16 GUID code units parallel to `occurrence_guids`.
    occurrence_guid_offsets: Vec<u64>,
    /// Four ordered identity GUIDs following a class-390 occurrence path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identity_guids: Vec<DesignRelaxedGuidText>,
    /// Byte offsets parallel to `identity_guids`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identity_guid_offsets: Vec<u64>,
}

impl DesignAssemblyOperandPath {
    pub(crate) fn try_new(
        link: DesignAssemblyOperandPathLink,
        record_index: u32,
        class_tag: DesignClassTag,
        byte_offset: u64,
        occurrence_guids: Vec<Located<DesignRelaxedGuidText>>,
        identity_guids: Vec<Located<DesignRelaxedGuidText>>,
    ) -> Result<Self, String> {
        let legacy_pair =
            class_tag.as_str() == "386" && matches!(link.locator_class_tag.as_str(), "363" | "378");
        if occurrence_guids.is_empty() || (legacy_pair && occurrence_guids.len() != 1) {
            return Err("occurrence_guids has invalid assembly path arity".into());
        }
        let identity_arity = if legacy_pair {
            identity_guids.len() == 1
        } else {
            match class_tag.as_str() {
                "294" | "299" | "307" | "386" | "390" | "412" => identity_guids.len() == 4,
                "329" => matches!(identity_guids.len(), 0 | 4),
                "330" => !identity_guids.is_empty() && identity_guids.len().is_multiple_of(4),
                _ => false,
            }
        };
        if !identity_arity {
            return Err("identity_guids has invalid assembly path class arity".into());
        }
        for (field, guids) in [
            ("occurrence_guids", &occurrence_guids),
            ("identity_guids", &identity_guids),
        ] {
            if !guids.windows(2).all(|pair| pair[0].offset < pair[1].offset) {
                return Err(format!("{field} offsets must strictly increase"));
            }
            if !legacy_pair
                && class_tag.as_str() != "412"
                && guids.iter().any(|guid| guid.offset <= byte_offset)
            {
                return Err(format!("{field} offsets must follow byte_offset"));
            }
        }
        Ok(Self {
            link,
            record_index,
            class_tag,
            byte_offset,
            occurrence_guids,
            identity_guids,
        })
    }
    pub(crate) fn link(&self) -> &DesignAssemblyOperandPathLink {
        &self.link
    }
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn occurrence_guids(&self) -> &[Located<DesignRelaxedGuidText>] {
        &self.occurrence_guids
    }
    pub(crate) fn identity_guids(&self) -> &[Located<DesignRelaxedGuidText>] {
        &self.identity_guids
    }
    pub(crate) fn try_append(self, continuation: Self) -> Result<Self, String> {
        if self.class_tag.as_str() != "330" || continuation.class_tag.as_str() != "330" {
            return Err("assembly path continuations require class_tag 330".into());
        }
        let mut occurrences = self.occurrence_guids;
        occurrences.extend(continuation.occurrence_guids);
        let mut identities = self.identity_guids;
        identities.extend(continuation.identity_guids);
        Self::try_new(
            self.link,
            self.record_index,
            self.class_tag,
            self.byte_offset,
            occurrences,
            identities,
        )
    }
}

impl TryFrom<DesignAssemblyOperandPathWire> for DesignAssemblyOperandPath {
    type Error = String;

    fn try_from(wire: DesignAssemblyOperandPathWire) -> Result<Self, Self::Error> {
        if wire.occurrence_guids.len() != wire.occurrence_guid_offsets.len() {
            return Err(
                "occurrence_guids and occurrence_guid_offsets must have equal lengths".into(),
            );
        }
        if wire.identity_guids.len() != wire.identity_guid_offsets.len() {
            return Err("identity_guids and identity_guid_offsets must have equal lengths".into());
        }
        Self::try_new(
            wire.link,
            wire.record_index,
            wire.class_tag.try_into()?,
            wire.byte_offset,
            wire.occurrence_guids
                .into_iter()
                .zip(wire.occurrence_guid_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            wire.identity_guids
                .into_iter()
                .zip(wire.identity_guid_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
        )
    }
}

impl From<DesignAssemblyOperandPath> for DesignAssemblyOperandPathWire {
    fn from(path: DesignAssemblyOperandPath) -> Self {
        let (occurrence_guids, occurrence_guid_offsets) = path
            .occurrence_guids
            .into_iter()
            .map(|guid| (guid.value, guid.offset))
            .unzip();
        let (identity_guids, identity_guid_offsets) = path
            .identity_guids
            .into_iter()
            .map(|guid| (guid.value, guid.offset))
            .unzip();
        Self {
            link: path.link,
            record_index: path.record_index,
            class_tag: path.class_tag.into(),
            byte_offset: path.byte_offset,
            occurrence_guids,
            occurrence_guid_offsets,
            identity_guids,
            identity_guid_offsets,
        }
    }
}

/// One exact native qualifier for an assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignAssemblyOperandQualifier {
    /// Ordered occurrence path carried by a locator graph.
    OccurrencePath {
        /// Exact path and its native reference chain.
        path: DesignAssemblyOperandPath,
    },
    /// Pathless target carried by an axial selector graph.
    AxialTarget {
        /// Exact pathless target.
        target: DesignAssemblyAxialOperandTarget,
    },
    /// Datum connector owned directly by the current document root.
    JointOrigin {
        /// Referenced `JointOrigin` feature scope.
        scope_record_index: u32,
        /// Dynamic class of the scope's primary indexed header.
        class_tag: DesignClassTag,
        /// Byte offset of the primary indexed header.
        byte_offset: u64,
        /// Dynamic class of the paired indexed header.
        paired_class_tag: DesignClassTag,
        /// Byte offset of the paired indexed header.
        paired_byte_offset: u64,
    },
}

impl DesignAssemblyOperandQualifier {
    /// Return the occurrence path when this qualifier carries one.
    pub(crate) fn occurrence_path(&self) -> Option<&DesignAssemblyOperandPath> {
        match self {
            Self::OccurrencePath { path } => Some(path),
            Self::AxialTarget { .. } | Self::JointOrigin { .. } => None,
        }
    }
}

/// One operand frame embedded by an assembly-operation scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignAssemblyOperandFrame {
    /// Construction record referenced by the operand.
    pub reference_record_index: u32,
    /// Byte offset of `reference_record_index`.
    pub reference_offset: u64,
    /// Row-major operand-local-to-model transform.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the first transform scalar.
    pub transform_offset: u64,
}

#[cfg(test)]
mod tests;
