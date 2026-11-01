// SPDX-License-Identifier: Apache-2.0
//! Construction operand groups, frames, paths, identities and tracking paths.

use super::extrude_selection::DesignExtrudeFaceEncoding;
use super::extrude_selection::DesignExtrudeFaceRole;
use super::extrude_selection::DesignExtrudeOperandRole;
use super::extrude_selection::DesignExtrudeOperandRoleTag;
use super::extrude_selection::DesignOperandRole;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::serde_column::SliceColumn;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;

/// A construction role classified in its owning scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignConstructionOperandRole {
    /// A source role without an Extrude classification.
    Other(DesignOperandRole),
    /// Extrude body operand run A.
    ExtrudeBodiesA,
    /// Extrude body operand run B.
    ExtrudeBodiesB,
    /// Extrude profile operand run.
    ExtrudeProfile,
    /// An ordered Extrude face operand run.
    ExtrudeFaces {
        /// Source encoding admitted by the Extrude scope.
        encoding: DesignExtrudeFaceEncoding,
        /// Use assigned by the face run's position in the scope.
        usage: DesignExtrudeFaceRole,
    },
}

impl DesignConstructionOperandRole {
    /// The source role code.
    fn source(self) -> DesignOperandRole {
        match self {
            Self::Other(role) => role,
            Self::ExtrudeBodiesA => DesignOperandRole::BODIES_A,
            Self::ExtrudeBodiesB => DesignOperandRole::BODIES_B,
            Self::ExtrudeProfile => DesignOperandRole::PROFILE,
            Self::ExtrudeFaces { encoding, .. } => match encoding {
                DesignExtrudeFaceEncoding::Faces => DesignOperandRole::FACES,
                DesignExtrudeFaceEncoding::SelectedStart => DesignOperandRole::ROLE_0X5,
                DesignExtrudeFaceEncoding::LegacyTermination => DesignOperandRole::ROLE_0X12,
            },
        }
    }

    /// The Extrude role of a scope-classified source encoding.
    fn extrude(self) -> Option<DesignExtrudeOperandRole> {
        match self {
            Self::Other(_) => None,
            Self::ExtrudeBodiesA | Self::ExtrudeBodiesB => Some(DesignExtrudeOperandRole::Bodies),
            Self::ExtrudeProfile => Some(DesignExtrudeOperandRole::Profile),
            Self::ExtrudeFaces { usage, .. } => Some(DesignExtrudeOperandRole::Faces(usage)),
        }
    }
}

/// Construction-operand group owned by a feature scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandGroupSerde")]
pub(crate) struct DesignConstructionOperandGroup {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Position in the scope reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: Option<u64>,
    /// Per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Ordered operand-record references.
    members: Vec<Located<u32, Option<u64>>>,
    /// Ordered unresolved-edge records whose run terminates at this group's identity.
    pub(crate) lost_edge_references: Vec<String>,
    /// Exact framing of the operand-member run and its auxiliary fields.
    pub(crate) frame: DesignConstructionOperandGroupFrame,
    /// Source role classified in its owning scope.
    pub(crate) operand_role: DesignConstructionOperandRole,
    /// Per-file dynamic paired class tag.
    paired_class_tag: DesignClassTag,
    /// Same-index paired-header byte offset.
    pub(crate) paired_byte_offset: Option<u64>,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_GROUP_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandGroup {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_GROUP_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            members: self.members.clone(),
            lost_edge_references: self.lost_edge_references.clone(),
            frame: self.frame.clone(),
            operand_role: self.operand_role,
            paired_class_tag: self.paired_class_tag.clone(),
            paired_byte_offset: self.paired_byte_offset,
        }
    }
}

impl Serialize for DesignConstructionOperandGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            id: &'a str,
            scope_record_index: u32,
            scope_reference_ordinal: u32,
            record_index: u32,
            class_tag: &'a str,
            members: SliceColumn<'a, Located<u32, Option<u64>>, u32>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            lost_edge_references: &'a Vec<String>,
            frame: &'a DesignConstructionOperandGroupFrame,
            role: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            extrude_role: Option<DesignExtrudeOperandRoleTag>,
            #[serde(skip_serializing_if = "Option::is_none")]
            extrude_face_role: Option<DesignExtrudeFaceRole>,
            paired_class_tag: &'a str,
        }
        let (extrude_role, extrude_face_role) = match self.operand_role.extrude() {
            Some(DesignExtrudeOperandRole::Bodies) => {
                (Some(DesignExtrudeOperandRoleTag::Bodies), None)
            }
            Some(DesignExtrudeOperandRole::Profile) => {
                (Some(DesignExtrudeOperandRoleTag::Profile), None)
            }
            Some(DesignExtrudeOperandRole::Faces(face_role)) => {
                (Some(DesignExtrudeOperandRoleTag::Faces), Some(face_role))
            }
            None => (None, None),
        };
        BorrowedWire {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            class_tag: self.class_tag.as_str(),
            members: SliceColumn::new(&self.members, |member| member.value),
            lost_edge_references: &self.lost_edge_references,
            frame: &self.frame,
            role: self.role().raw(),
            extrude_role,
            extrude_face_role,
            paired_class_tag: self.paired_class_tag.as_str(),
        }
        .serialize(serializer)
    }
}

/// Unchecked construction-group input.
pub(crate) struct DesignConstructionOperandGroupDraft {
    pub(crate) id: String,
    pub(crate) scope_record_index: u32,
    pub(crate) scope_reference_ordinal: u32,
    pub(crate) record_index: u32,
    pub(crate) byte_offset: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) members: Vec<Located<u32>>,
    pub(crate) lost_edge_references: Vec<String>,
    pub(crate) frame: DesignConstructionOperandGroupFrame,
    pub(crate) operand_role: DesignConstructionOperandRole,
    pub(crate) role_offset: u64,
    pub(crate) paired_class_tag: DesignClassTag,
    pub(crate) paired_byte_offset: u64,
}

impl TryFrom<DesignConstructionOperandGroupDraft> for DesignConstructionOperandGroup {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandGroupDraft) -> Result<Self, Self::Error> {
        Self::check_members(&draft.members)?;
        if Some(draft.role_offset) != draft.frame.role_offset() {
            return Err("role_offset must precede opaque_index_offset by 18 bytes".into());
        }
        Ok(Self {
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            scope_reference_ordinal: draft.scope_reference_ordinal,
            record_index: draft.record_index,
            byte_offset: Some(draft.byte_offset),
            class_tag: draft.class_tag,
            members: draft
                .members
                .into_iter()
                .map(|row| Located {
                    value: row.value,
                    offset: Some(row.offset),
                })
                .collect(),
            lost_edge_references: draft.lost_edge_references,
            frame: draft.frame,
            operand_role: draft.operand_role,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: Some(draft.paired_byte_offset),
        })
    }
}

impl DesignConstructionOperandGroup {
    fn check_members(members: &[Located<u32>]) -> Result<(), String> {
        if members.windows(2).any(|pair| {
            pair[0]
                .offset
                .checked_add(11)
                .is_none_or(|next| pair[1].offset < next)
        }) {
            return Err("construction member offsets must have a stride of at least 11".into());
        }
        Ok(())
    }

    /// Ordered operand references with checked strides.
    pub(crate) fn members(&self) -> &[Located<u32, Option<u64>>] {
        &self.members
    }

    /// Checked replacement of the operand-reference run.
    #[cfg(test)]
    pub(crate) fn try_set_members(&mut self, members: Vec<Located<u32>>) -> Result<(), String> {
        Self::check_members(&members)?;
        self.members = members
            .into_iter()
            .map(|row| Located {
                value: row.value,
                offset: Some(row.offset),
            })
            .collect();
        Ok(())
    }

    /// Role offset derived from the opaque-index location.
    pub(crate) fn role_offset(&self) -> Option<u64> {
        self.frame.role_offset()
    }

    /// The source role code.
    pub(crate) fn role(&self) -> DesignOperandRole {
        self.operand_role.source()
    }

    /// The Extrude role derived from the scope-classified source encoding.
    pub(crate) fn extrude_role(&self) -> Option<DesignExtrudeOperandRole> {
        self.operand_role.extrude()
    }

    pub(crate) fn extrude_face_role(&self) -> Option<DesignExtrudeFaceRole> {
        match self.extrude_role() {
            Some(DesignExtrudeOperandRole::Faces(role)) => Some(role),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandGroupSerde {
    pub(crate) id: String,
    pub(crate) scope_record_index: u32,
    pub(crate) scope_reference_ordinal: u32,
    pub(crate) record_index: u32,
    pub(crate) class_tag: String,
    pub(crate) members: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) lost_edge_references: Vec<String>,
    pub(crate) frame: DesignConstructionOperandGroupFrame,
    pub(crate) role: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_role"
    )]
    pub(crate) extrude_role: Option<DesignExtrudeOperandRoleTag>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_face_role"
    )]
    pub(crate) extrude_face_role: Option<DesignExtrudeFaceRole>,
    pub(crate) paired_class_tag: String,
}

impl TryFrom<DesignConstructionOperandGroupSerde> for DesignConstructionOperandGroup {
    type Error = String;

    fn try_from(wire: DesignConstructionOperandGroupSerde) -> Result<Self, Self::Error> {
        let role = DesignOperandRole::from_raw(wire.role);
        let operand_role = match (role, wire.extrude_role, wire.extrude_face_role) {
            (DesignOperandRole::BODIES_A, Some(DesignExtrudeOperandRoleTag::Bodies), None) => {
                DesignConstructionOperandRole::ExtrudeBodiesA
            }
            (DesignOperandRole::BODIES_B, Some(DesignExtrudeOperandRoleTag::Bodies), None) => {
                DesignConstructionOperandRole::ExtrudeBodiesB
            }
            (DesignOperandRole::PROFILE, Some(DesignExtrudeOperandRoleTag::Profile), None) => {
                DesignConstructionOperandRole::ExtrudeProfile
            }
            (role, Some(DesignExtrudeOperandRoleTag::Faces), Some(usage)) => {
                let encoding = match role {
                    DesignOperandRole::FACES => DesignExtrudeFaceEncoding::Faces,
                    DesignOperandRole::ROLE_0X5 => DesignExtrudeFaceEncoding::SelectedStart,
                    DesignOperandRole::ROLE_0X12 => DesignExtrudeFaceEncoding::LegacyTermination,
                    _ => return Err("role does not encode a faces extrude_role".into()),
                };
                DesignConstructionOperandRole::ExtrudeFaces { encoding, usage }
            }
            (role, None, None) => DesignConstructionOperandRole::Other(role),
            _ => return Err("role, extrude_role, and extrude_face_role disagree".into()),
        };
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: None,
            class_tag: wire.class_tag.try_into()?,
            members: wire
                .members
                .into_iter()
                .map(|value| Located {
                    value,
                    offset: None,
                })
                .collect(),
            lost_edge_references: wire.lost_edge_references,
            frame: wire.frame,
            operand_role,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: None,
        })
    }
}

#[cfg(test)]
impl From<DesignConstructionOperandGroup> for DesignConstructionOperandGroupSerde {
    fn from(group: DesignConstructionOperandGroup) -> Self {
        let members = group
            .members
            .into_iter()
            .map(|member| member.value)
            .collect();
        let (extrude_role, extrude_face_role) = match group.operand_role.extrude() {
            Some(DesignExtrudeOperandRole::Bodies) => {
                (Some(DesignExtrudeOperandRoleTag::Bodies), None)
            }
            Some(DesignExtrudeOperandRole::Profile) => {
                (Some(DesignExtrudeOperandRoleTag::Profile), None)
            }
            Some(DesignExtrudeOperandRole::Faces(face_role)) => {
                (Some(DesignExtrudeOperandRoleTag::Faces), Some(face_role))
            }
            None => (None, None),
        };
        Self {
            id: group.id,
            scope_record_index: group.scope_record_index,
            scope_reference_ordinal: group.scope_reference_ordinal,
            record_index: group.record_index,
            class_tag: group.class_tag.into(),
            members,
            lost_edge_references: group.lost_edge_references,
            frame: group.frame,
            role: group.operand_role.source().raw(),
            extrude_role,
            extrude_face_role,
            paired_class_tag: group.paired_class_tag.into(),
        }
    }
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandGroupFrameWire")]
pub(crate) struct DesignConstructionOperandGroupFrame {
    /// Byte offset of the member count.
    pub(crate) member_count_offset: Option<u64>,
    /// Auxiliary records named by the two optional references that follow the
    /// member run; an absent reference contributes no entry.
    pub(crate) auxiliary_records: Vec<Located<u32, Option<u64>>>,
    /// Exact selection-path records selected by the optional references.
    auxiliary_paths: Vec<DesignConstructionOperandPath>,
    /// Indexed records named by the counted trailing-reference run. The target
    /// grammar is selected by the owning operand family: persistent-selection
    /// groups name identity wrappers and placed-selection groups name affine
    /// transforms.
    trailing_records: Option<Located<u32, Option<u64>>>,
    /// Exact affine-transform records selected from the trailing-reference
    /// run. Other trailing records remain represented by their indices and
    /// offsets and can select another typed grammar.
    trailing_transforms: Vec<DesignConstructionOperandTransform>,
    /// Exact dual-transform records selected from the trailing-reference run.
    trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    /// Exact compact flag records selected from the trailing-reference run.
    trailing_flags: Vec<DesignConstructionOperandFlag>,
    /// Opaque ordinal: nonzero and below 256, repeated after `opaque_scalar` in
    /// every container generation but one.
    pub(crate) opaque_index: NonZeroU32,
    /// Byte offset of the first `opaque_index` copy.
    opaque_index_offset: Option<u64>,
    /// Opaque scalar.
    opaque_scalar: cadmpeg_ir::scalar::NonNegativeReal,
    /// Boolean tail variant.
    pub(crate) variant: bool,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_FRAME_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandGroupFrame {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_FRAME_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            member_count_offset: self.member_count_offset,
            auxiliary_records: self.auxiliary_records.clone(),
            auxiliary_paths: self.auxiliary_paths.clone(),
            trailing_records: self.trailing_records,
            trailing_transforms: self.trailing_transforms.clone(),
            trailing_dual_transforms: self.trailing_dual_transforms.clone(),
            trailing_flags: self.trailing_flags.clone(),
            opaque_index: self.opaque_index,
            opaque_index_offset: self.opaque_index_offset,
            opaque_scalar: self.opaque_scalar,
            variant: self.variant,
        }
    }
}

impl Serialize for DesignConstructionOperandGroupFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            auxiliary_record_indices: SliceColumn<'a, Located<u32, Option<u64>>, u32>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            auxiliary_paths: &'a Vec<DesignConstructionOperandPath>,
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            trailing_record_indices: SliceColumn<'a, Located<u32, Option<u64>>, u32>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            trailing_transforms: &'a Vec<DesignConstructionOperandTransform>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            trailing_dual_transforms: &'a Vec<DesignConstructionOperandDualTransform>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            trailing_flags: &'a Vec<DesignConstructionOperandFlag>,
            opaque_index: u32,
            opaque_scalar: f64,
            variant: bool,
        }
        let trailing = self.trailing_records.as_slice();
        BorrowedWire {
            auxiliary_record_indices: SliceColumn::new(&self.auxiliary_records, |row| row.value),
            auxiliary_paths: &self.auxiliary_paths,
            trailing_record_indices: SliceColumn::new(trailing, |row| row.value),
            trailing_transforms: &self.trailing_transforms,
            trailing_dual_transforms: &self.trailing_dual_transforms,
            trailing_flags: &self.trailing_flags,
            opaque_index: self.opaque_index.get(),
            opaque_scalar: self.opaque_scalar().get(),
            variant: self.variant,
        }
        .serialize(serializer)
    }
}

/// Construction-frame fields before aggregate validation.
pub(crate) struct DesignConstructionOperandGroupFrameDraft<T = f64, I = u32> {
    pub(crate) member_count_offset: u64,
    pub(crate) auxiliary_records: Vec<Located<u32>>,
    pub(crate) auxiliary_paths: Vec<DesignConstructionOperandPath>,
    pub(crate) trailing_records: Vec<Located<u32>>,
    pub(crate) trailing_transforms: Vec<DesignConstructionOperandTransform>,
    pub(crate) trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    pub(crate) trailing_flags: Vec<DesignConstructionOperandFlag>,
    pub(crate) opaque_index: I,
    pub(crate) opaque_index_offset: u64,
    pub(crate) opaque_scalar: T,
    pub(crate) opaque_scalar_offset: u64,
    pub(crate) variant: bool,
}

impl TryFrom<DesignConstructionOperandGroupFrameDraft> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandGroupFrameDraft) -> Result<Self, Self::Error> {
        if draft.trailing_records.len() > 1 {
            return Err("construction frame permits at most one trailing record".into());
        }
        let opaque_index =
            NonZeroU32::new(draft.opaque_index).ok_or("opaque_index must be nonzero")?;
        let opaque_scalar = cadmpeg_ir::scalar::NonNegativeReal::new(draft.opaque_scalar)
            .ok_or("opaque_scalar must be finite and nonnegative")?;
        Self::from_parts(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: draft.member_count_offset,
            auxiliary_records: draft.auxiliary_records,
            auxiliary_paths: draft.auxiliary_paths,
            trailing_records: draft.trailing_records,
            trailing_transforms: draft.trailing_transforms,
            trailing_dual_transforms: draft.trailing_dual_transforms,
            trailing_flags: draft.trailing_flags,
            opaque_index,
            opaque_index_offset: draft.opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: draft.opaque_scalar_offset,
            variant: draft.variant,
        })
    }
}

impl DesignConstructionOperandGroupFrame {
    pub(crate) fn from_parts(
        draft: DesignConstructionOperandGroupFrameDraft<
            cadmpeg_ir::scalar::NonNegativeReal,
            NonZeroU32,
        >,
    ) -> Result<Self, String> {
        if draft.trailing_records.len() > 1 {
            return Err("construction frame permits at most one trailing record".into());
        }
        if draft.opaque_index_offset < 18
            || draft.opaque_index_offset.checked_add(4) != Some(draft.opaque_scalar_offset)
        {
            return Err("opaque_index_offset and opaque_scalar_offset must follow the role by 18 and 22 bytes".into());
        }
        distinct_construction_records(
            "auxiliary_paths",
            draft
                .auxiliary_paths
                .iter()
                .map(DesignConstructionOperandPath::record_index),
        )?;
        distinct_construction_records(
            "trailing_transforms",
            draft
                .trailing_transforms
                .iter()
                .map(DesignConstructionOperandTransform::record_index),
        )?;
        distinct_construction_records(
            "trailing_dual_transforms",
            draft
                .trailing_dual_transforms
                .iter()
                .map(|record| record.record_index),
        )?;
        distinct_construction_records(
            "trailing_flags",
            draft
                .trailing_flags
                .iter()
                .map(|record| record.record_index),
        )?;
        Ok(Self {
            member_count_offset: Some(draft.member_count_offset),
            auxiliary_records: draft
                .auxiliary_records
                .into_iter()
                .map(|row| Located {
                    value: row.value,
                    offset: Some(row.offset),
                })
                .collect(),
            auxiliary_paths: draft.auxiliary_paths,
            trailing_records: draft
                .trailing_records
                .into_iter()
                .next()
                .map(|row| Located {
                    value: row.value,
                    offset: Some(row.offset),
                }),
            trailing_transforms: draft.trailing_transforms,
            trailing_dual_transforms: draft.trailing_dual_transforms,
            trailing_flags: draft.trailing_flags,
            opaque_index: draft.opaque_index,
            opaque_index_offset: Some(draft.opaque_index_offset),
            opaque_scalar: draft.opaque_scalar,
            variant: draft.variant,
        })
    }
}

fn distinct_construction_records(
    field: &str,
    records: impl Iterator<Item = u32>,
) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for record in records {
        if !seen.insert(record) {
            return Err(format!("{field} record_index values must be distinct"));
        }
    }
    Ok(())
}

impl DesignConstructionOperandGroupFrame {
    /// Role offset derived from the opaque-index location.
    fn role_offset(&self) -> Option<u64> {
        self.opaque_index_offset?.checked_sub(18)
    }
    /// First opaque-index location.
    pub(crate) fn opaque_index_offset(&self) -> Option<u64> {
        self.opaque_index_offset
    }
    /// Scalar location following the opaque index.
    pub(crate) fn opaque_scalar_offset(&self) -> Option<u64> {
        self.opaque_index_offset?.checked_add(4)
    }
    /// Opaque scalar.
    pub(crate) fn opaque_scalar(&self) -> cadmpeg_ir::scalar::NonNegativeReal {
        self.opaque_scalar
    }
    /// Zero or one trailing reference.
    pub(crate) fn trailing_records(&self) -> &[Located<u32, Option<u64>>] {
        self.trailing_records.as_slice()
    }
    /// Distinct auxiliary path records.
    pub(crate) fn auxiliary_paths(&self) -> &[DesignConstructionOperandPath] {
        &self.auxiliary_paths
    }
    /// Checked replacement of auxiliary path records.
    pub(crate) fn try_set_auxiliary_paths(
        &mut self,
        records: Vec<DesignConstructionOperandPath>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "auxiliary_paths",
            records
                .iter()
                .map(DesignConstructionOperandPath::record_index),
        )?;
        self.auxiliary_paths = records;
        Ok(())
    }
    /// Distinct trailing affine transforms.
    pub(crate) fn trailing_transforms(&self) -> &[DesignConstructionOperandTransform] {
        &self.trailing_transforms
    }
    /// Checked replacement of trailing affine transforms.
    pub(crate) fn try_set_trailing_transforms(
        &mut self,
        records: Vec<DesignConstructionOperandTransform>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_transforms",
            records
                .iter()
                .map(DesignConstructionOperandTransform::record_index),
        )?;
        self.trailing_transforms = records;
        Ok(())
    }
    /// Distinct trailing dual transforms.
    pub(crate) fn trailing_dual_transforms(&self) -> &[DesignConstructionOperandDualTransform] {
        &self.trailing_dual_transforms
    }
    /// Checked replacement of trailing dual transforms.
    pub(crate) fn try_set_trailing_dual_transforms(
        &mut self,
        records: Vec<DesignConstructionOperandDualTransform>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_dual_transforms",
            records.iter().map(|record| record.record_index),
        )?;
        self.trailing_dual_transforms = records;
        Ok(())
    }
    /// Distinct trailing flag records.
    pub(crate) fn trailing_flags(&self) -> &[DesignConstructionOperandFlag] {
        &self.trailing_flags
    }
    /// Checked replacement of trailing flag records.
    pub(crate) fn try_set_trailing_flags(
        &mut self,
        records: Vec<DesignConstructionOperandFlag>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_flags",
            records.iter().map(|record| record.record_index),
        )?;
        self.trailing_flags = records;
        Ok(())
    }
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandGroupFrameWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) auxiliary_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) auxiliary_paths: Vec<DesignConstructionOperandPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) trailing_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) trailing_transforms: Vec<DesignConstructionOperandTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) trailing_flags: Vec<DesignConstructionOperandFlag>,
    pub(crate) opaque_index: u32,
    pub(crate) opaque_scalar: f64,
    pub(crate) variant: bool,
}
impl TryFrom<DesignConstructionOperandGroupFrameWire> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandGroupFrameWire) -> Result<Self, String> {
        if wire.trailing_record_indices.len() > 1 {
            return Err("construction frame permits at most one trailing record".into());
        }
        distinct_construction_records(
            "auxiliary_paths",
            wire.auxiliary_paths
                .iter()
                .map(DesignConstructionOperandPath::record_index),
        )?;
        distinct_construction_records(
            "trailing_transforms",
            wire.trailing_transforms
                .iter()
                .map(DesignConstructionOperandTransform::record_index),
        )?;
        distinct_construction_records(
            "trailing_dual_transforms",
            wire.trailing_dual_transforms
                .iter()
                .map(|record| record.record_index),
        )?;
        distinct_construction_records(
            "trailing_flags",
            wire.trailing_flags.iter().map(|record| record.record_index),
        )?;
        Ok(Self {
            member_count_offset: None,
            auxiliary_records: wire
                .auxiliary_record_indices
                .into_iter()
                .map(|value| Located {
                    value,
                    offset: None,
                })
                .collect(),
            auxiliary_paths: wire.auxiliary_paths,
            trailing_records: wire
                .trailing_record_indices
                .into_iter()
                .next()
                .map(|value| Located {
                    value,
                    offset: None,
                }),
            trailing_transforms: wire.trailing_transforms,
            trailing_dual_transforms: wire.trailing_dual_transforms,
            trailing_flags: wire.trailing_flags,
            opaque_index: NonZeroU32::new(wire.opaque_index)
                .ok_or("opaque_index must be nonzero")?,
            opaque_index_offset: None,
            opaque_scalar: cadmpeg_ir::scalar::NonNegativeReal::new(wire.opaque_scalar)
                .ok_or("opaque_scalar must be finite and nonnegative")?,
            variant: wire.variant,
        })
    }
}
#[cfg(test)]
impl From<DesignConstructionOperandGroupFrame> for DesignConstructionOperandGroupFrameWire {
    fn from(frame: DesignConstructionOperandGroupFrame) -> Self {
        Self {
            auxiliary_record_indices: frame
                .auxiliary_records
                .iter()
                .map(|row| row.value)
                .collect(),
            auxiliary_paths: frame.auxiliary_paths,
            trailing_record_indices: frame.trailing_records.iter().map(|row| row.value).collect(),
            trailing_transforms: frame.trailing_transforms,
            trailing_dual_transforms: frame.trailing_dual_transforms,
            trailing_flags: frame.trailing_flags,
            opaque_index: frame.opaque_index.get(),
            opaque_scalar: frame.opaque_scalar.get(),
            variant: frame.variant,
        }
    }
}

/// Compact boolean record named by a construction-operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandFlag {
    /// Indexed flag-record identity.
    pub(crate) record_index: u32,
    /// Flag-record header byte offset.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
    /// Per-file dynamic flag-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Stored boolean value.
    pub(crate) value: bool,
    /// Byte offset of the stored boolean.
    #[serde(skip)]
    pub(crate) value_offset: Option<u64>,
}

/// Affine placement named by a construction-operand group's trailing run.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandTransformContent")]
pub(crate) struct DesignConstructionOperandTransform {
    record_index: u32,
    frame: Option<crate::records::frame_chain::RecordFrameChain>,
    following_record_index: u32,
    /// Per-file dynamic transform-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Row-major local-to-model affine transform.
    pub(crate) transform: SketchPlacementMatrix,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_TRANSFORM_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandTransform {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_TRANSFORM_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            record_index: self.record_index,
            frame: self.frame,
            following_record_index: self.following_record_index,
            class_tag: self.class_tag.clone(),
            transform: self.transform,
            following_class_tag: self.following_class_tag.clone(),
        }
    }
}

impl Serialize for DesignConstructionOperandTransform {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            record_index: u32,
            class_tag: &'a DesignClassTag,
            transform: SketchPlacementMatrix,
            following_record_index: u32,
            following_class_tag: &'a DesignClassTag,
        }
        WireRef {
            record_index: self.record_index(),
            class_tag: &self.class_tag,
            transform: self.transform,
            following_record_index: self.following_record_index(),
            following_class_tag: &self.following_class_tag,
        }
        .serialize(serializer)
    }
}

impl DesignConstructionOperandTransform {
    pub(crate) fn try_new(draft: DesignConstructionOperandTransformDraft) -> Result<Self, String> {
        if draft.following_record_index <= draft.record_index {
            return Err("following_record_index must exceed record_index".into());
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            0,
            152,
        )?;
        let value = Self {
            record_index: draft.record_index,
            frame: Some(frame),
            following_record_index: draft.following_record_index,
            class_tag: draft.class_tag,
            transform: draft.transform,
            following_class_tag: draft.following_class_tag,
        };
        if value.transform_offset() != Some(draft.transform_offset) {
            return Err("transform_offset disagrees with frame layout".into());
        }
        if value.following_byte_offset() != Some(draft.following_byte_offset) {
            return Err("following_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignConstructionOperandTransformDraft, String> {
        let record_index = self.record_index();
        let byte_offset = self
            .byte_offset()
            .ok_or("document transform has no source byte_offset")?;
        let following_record_index = self.following_record_index();
        let transform_offset = self
            .transform_offset()
            .ok_or("document transform has no source transform_offset")?;
        let following_byte_offset = self
            .following_byte_offset()
            .ok_or("document transform has no source following_byte_offset")?;
        Ok(DesignConstructionOperandTransformDraft {
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            transform: self.transform,
            transform_offset,
            following_record_index,
            following_byte_offset,
            following_class_tag: self.following_class_tag,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(0))
    }
    pub(crate) fn transform_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(22))
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.following_record_index
    }
    pub(crate) fn following_byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(152))
    }
}

/// Unadmitted `DesignConstructionOperandTransform` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignConstructionOperandTransformDraft {
    /// Indexed transform-record identity.
    pub(crate) record_index: u32,
    /// Transform-record header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Row-major local-to-model affine transform.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) transform_offset: u64,
    /// Indexed record immediately following the transform.
    pub(crate) following_record_index: u32,
    /// Following-record header byte offset.
    pub(crate) following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

impl TryFrom<DesignConstructionOperandTransformDraft> for DesignConstructionOperandTransform {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandTransformDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}

/// Typed placement without a source encoding or source positions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandTransformContent {
    pub(crate) record_index: u32,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) transform: SketchPlacementMatrix,
    pub(crate) following_record_index: u32,
    pub(crate) following_class_tag: DesignClassTag,
}
impl TryFrom<DesignConstructionOperandTransformContent> for DesignConstructionOperandTransform {
    type Error = String;
    fn try_from(content: DesignConstructionOperandTransformContent) -> Result<Self, String> {
        if content.following_record_index <= content.record_index {
            return Err("following_record_index must exceed record_index".into());
        }
        Ok(Self {
            record_index: content.record_index,
            frame: None,
            following_record_index: content.following_record_index,
            class_tag: content.class_tag,
            transform: content.transform,
            following_class_tag: content.following_class_tag,
        })
    }
}
#[cfg(test)]
impl From<DesignConstructionOperandTransform> for DesignConstructionOperandTransformContent {
    fn from(value: DesignConstructionOperandTransform) -> Self {
        Self {
            record_index: value.record_index(),
            following_record_index: value.following_record_index(),
            class_tag: value.class_tag,
            transform: value.transform,
            following_class_tag: value.following_class_tag,
        }
    }
}

/// Two ordered affine placements named by an operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandDualTransform {
    /// Indexed transform-record identity.
    pub(crate) record_index: u32,
    /// Transform-record header byte offset.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
    /// Per-file dynamic transform-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// First row-major affine transform.
    pub(crate) first_transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    #[serde(skip)]
    pub(crate) first_transform_offset: Option<u64>,
    /// Second row-major affine transform.
    pub(crate) second_transform: SketchPlacementMatrix,
    /// Byte offset of the second matrix scalar.
    #[serde(skip)]
    pub(crate) second_transform_offset: Option<u64>,
}

/// One persistent-entity step in a construction operand's selection path.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandPathWire")]
pub(crate) struct DesignConstructionOperandPath {
    record_index: u32,
    frame: Option<crate::records::frame_chain::RecordFrameChain>,
    /// Per-file dynamic path-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Persistent entity identity carried by this path step.
    pub(crate) entity_ref: u64,
    /// Transform or compact selection-path layout.
    placement: DesignConstructionPathPlacement,
    /// Owning feature-scope record.
    pub(crate) scope_record_index: u32,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_PATH_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandPath {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_PATH_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            record_index: self.record_index,
            frame: self.frame,
            class_tag: self.class_tag.clone(),
            entity_ref: self.entity_ref,
            placement: self.placement,
            scope_record_index: self.scope_record_index,
            following_class_tag: self.following_class_tag.clone(),
        }
    }
}

impl Serialize for DesignConstructionOperandPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            record_index: u32,
            class_tag: &'a str,
            entity_ref: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform: Option<SketchPlacementMatrix>,
            #[serde(skip_serializing_if = "Option::is_none")]
            compact_variant: Option<bool>,
            scope_record_index: u32,
            nested_record_index: u32,
            following_record_index: u32,
            following_class_tag: &'a str,
        }
        let (transform, compact_variant) = match self.placement {
            DesignConstructionPathPlacement::Transform(transform) => (Some(transform), None),
            DesignConstructionPathPlacement::Compact(variant) => (None, Some(variant)),
        };
        WireRef {
            record_index: self.record_index(),
            class_tag: self.class_tag.as_str(),
            entity_ref: self.entity_ref,
            transform,
            compact_variant,
            scope_record_index: self.scope_record_index,
            nested_record_index: self.nested_record_index(),
            following_record_index: self.following_record_index(),
            following_class_tag: self.following_class_tag.as_str(),
        }
        .serialize(serializer)
    }
}

impl DesignConstructionOperandPath {
    pub(crate) fn try_new(draft: DesignConstructionOperandPathDraft) -> Result<Self, String> {
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            2,
            if matches!(
                draft.placement,
                DesignConstructionPathPlacement::Transform(_)
            ) {
                190
            } else {
                62
            },
        )?;
        let value = Self {
            record_index: draft.record_index,
            frame: Some(frame),
            class_tag: draft.class_tag,
            entity_ref: draft.entity_ref,
            placement: draft.placement,
            scope_record_index: draft.scope_record_index,
            following_class_tag: draft.following_class_tag,
        };
        if value.entity_ref_offset() != Some(draft.entity_ref_offset) {
            return Err("entity_ref_offset disagrees with frame layout".into());
        }
        if value.nested_record_index() != draft.nested_record_index {
            return Err("nested_record_index disagrees with frame layout".into());
        }
        if value.following_record_index() != draft.following_record_index {
            return Err("following_record_index disagrees with frame layout".into());
        }
        if value.scope_record_index_offset() != Some(draft.scope_record_index_offset) {
            return Err("scope_record_index_offset disagrees with frame layout".into());
        }
        if value.nested_record_index_offset() != Some(draft.nested_record_index_offset) {
            return Err("nested_record_index_offset disagrees with frame layout".into());
        }
        if value.following_byte_offset() != Some(draft.following_byte_offset) {
            return Err("following_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignConstructionOperandPathDraft, String> {
        let record_index = self.record_index();
        let byte_offset = self
            .byte_offset()
            .ok_or("document path has no source byte_offset")?;
        let entity_ref_offset = self
            .entity_ref_offset()
            .ok_or("document path has no source entity_ref_offset")?;
        let nested_record_index = self.nested_record_index();
        let following_record_index = self.following_record_index();
        let scope_record_index_offset = self
            .scope_record_index_offset()
            .ok_or("document path has no source scope_record_index_offset")?;
        let nested_record_index_offset = self
            .nested_record_index_offset()
            .ok_or("document path has no source nested_record_index_offset")?;
        let following_byte_offset = self
            .following_byte_offset()
            .ok_or("document path has no source following_byte_offset")?;
        Ok(DesignConstructionOperandPathDraft {
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            entity_ref: self.entity_ref,
            entity_ref_offset,
            placement: self.placement,
            scope_record_index: self.scope_record_index,
            scope_record_index_offset,
            nested_record_index,
            nested_record_index_offset,
            following_record_index,
            following_byte_offset,
            following_class_tag: self.following_class_tag,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(0))
    }
    fn entity_ref_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(22))
    }
    pub(crate) fn scope_record_index_offset(&self) -> Option<u64> {
        self.frame.map(|frame| {
            frame.offset(
                if matches!(
                    self.placement,
                    DesignConstructionPathPlacement::Transform(_)
                ) {
                    163
                } else {
                    35
                },
            )
        })
    }
    pub(crate) fn nested_record_index(&self) -> u32 {
        self.record_index + 2
    }
    pub(crate) fn nested_record_index_offset(&self) -> Option<u64> {
        self.scope_record_index_offset()?.checked_add(11)
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.record_index + 1
    }
    pub(crate) fn following_byte_offset(&self) -> Option<u64> {
        self.scope_record_index_offset()?.checked_add(27)
    }
}

/// Unadmitted `DesignConstructionOperandPath` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignConstructionOperandPathDraft {
    /// Indexed path-record identity.
    pub(crate) record_index: u32,
    /// Path-record header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Persistent entity identity carried by this path step.
    pub(crate) entity_ref: u64,
    /// Byte offset of `entity_ref`.
    pub(crate) entity_ref_offset: u64,
    /// Transform or compact selection-path layout.
    pub(crate) placement: DesignConstructionPathPlacement,
    /// Owning feature-scope record.
    pub(crate) scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    pub(crate) scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    pub(crate) nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    pub(crate) nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    pub(crate) following_record_index: u32,
    /// Following-record header byte offset.
    pub(crate) following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandPathWire {
    pub(crate) record_index: u32,
    pub(crate) class_tag: String,
    pub(crate) entity_ref: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    pub(crate) transform: Option<SketchPlacementMatrix>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_compact_variant"
    )]
    pub(crate) compact_variant: Option<bool>,
    pub(crate) scope_record_index: u32,
    pub(crate) nested_record_index: u32,
    pub(crate) following_record_index: u32,
    pub(crate) following_class_tag: String,
}
impl TryFrom<DesignConstructionOperandPathWire> for DesignConstructionOperandPath {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandPathWire) -> Result<Self, String> {
        if wire.record_index.checked_add(2) != Some(wire.nested_record_index) {
            return Err("nested_record_index disagrees with indexed path chain".into());
        }
        if wire.record_index.checked_add(1) != Some(wire.following_record_index) {
            return Err("following_record_index disagrees with indexed path chain".into());
        }
        let placement = match (wire.transform, wire.compact_variant) {
            (Some(transform), None) => DesignConstructionPathPlacement::Transform(transform),
            (None, Some(variant)) => DesignConstructionPathPlacement::Compact(variant),
            _ => {
                return Err(
                    "transform and compact_variant must select exactly one path placement".into(),
                );
            }
        };
        Ok(Self {
            record_index: wire.record_index,
            frame: None,
            class_tag: wire.class_tag.try_into()?,
            entity_ref: wire.entity_ref,
            placement,
            scope_record_index: wire.scope_record_index,
            following_class_tag: wire.following_class_tag.try_into()?,
        })
    }
}
#[cfg(test)]
impl From<DesignConstructionOperandPath> for DesignConstructionOperandPathWire {
    fn from(record: DesignConstructionOperandPath) -> Self {
        let (transform, compact_variant) = match record.placement {
            DesignConstructionPathPlacement::Transform(transform) => (Some(transform), None),
            DesignConstructionPathPlacement::Compact(variant) => (None, Some(variant)),
        };
        let record_index = record.record_index();
        let nested_record_index = record.nested_record_index();
        let following_record_index = record.following_record_index();
        Self {
            record_index,
            class_tag: record.class_tag.into(),
            entity_ref: record.entity_ref,
            transform,
            compact_variant,
            scope_record_index: record.scope_record_index,
            nested_record_index,
            following_record_index,
            following_class_tag: record.following_class_tag.into(),
        }
    }
}

/// Placement carried by a persistent-entity selection path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) enum DesignConstructionPathPlacement {
    Transform(SketchPlacementMatrix),
    Compact(bool),
}

/// Nested identity chain named by a construction-operand group.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandIdentityWire")]
pub(crate) struct DesignConstructionOperandIdentity {
    pub(crate) id: String,
    /// Semantic construction order within the containing Design stream.
    pub(crate) ordinal: u32,
    pub(crate) group_record_index: u32,
    wrappers: Vec<DesignIdentityWrapper>,
    following_record_index: u32,
    following_byte_offset: Option<u64>,
    following_class_tag: DesignClassTag,
    tracking_path: Option<DesignConstructionTrackingPath>,
    persistent_identity: Option<DesignConstructionPersistentIdentity>,
}
#[cfg(test)]
thread_local! {static CONSTRUCTION_IDENTITY_CLONE_COUNT:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
impl Clone for DesignConstructionOperandIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            ordinal: self.ordinal,
            group_record_index: self.group_record_index,
            wrappers: self.wrappers.clone(),
            following_record_index: self.following_record_index,
            following_byte_offset: self.following_byte_offset,
            following_class_tag: self.following_class_tag.clone(),
            tracking_path: self.tracking_path.clone(),
            persistent_identity: self.persistent_identity.clone(),
        }
    }
}
impl Serialize for DesignConstructionOperandIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            ordinal: u32,
            group_record_index: u32,
            wrapper_record_indices: SliceColumn<'a, DesignIdentityWrapper, u32>,
            wrapper_class_tags: SliceColumn<'a, DesignIdentityWrapper, &'a str>,
            following_record_index: u32,
            following_class_tag: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            tracking_path: Option<&'a DesignConstructionTrackingPath>,
            #[serde(skip_serializing_if = "Option::is_none")]
            persistent_identity: Option<&'a DesignConstructionPersistentIdentity>,
        }
        WireRef {
            id: &self.id,
            ordinal: self.ordinal,
            group_record_index: self.group_record_index,
            wrapper_record_indices: SliceColumn::new(&self.wrappers, |row| row.record_index),
            wrapper_class_tags: SliceColumn::new(&self.wrappers, |row| row.class_tag.as_str()),
            following_record_index: self.following_record_index,
            following_class_tag: self.following_class_tag.as_str(),
            tracking_path: self.tracking_path.as_ref(),
            persistent_identity: self.persistent_identity.as_ref(),
        }
        .serialize(serializer)
    }
}
impl DesignConstructionOperandIdentity {
    pub(crate) fn try_new(draft: DesignConstructionOperandIdentityDraft) -> Result<Self, String> {
        let mut indices = std::collections::HashSet::new();
        if !draft
            .wrappers
            .iter()
            .all(|wrapper| indices.insert(wrapper.record_index) && wrapper.byte_offset.is_some())
            || !draft.wrappers.windows(2).all(|pair| {
                pair[0]
                    .byte_offset
                    .and_then(|offset| offset.checked_add(24))
                    == pair[1].byte_offset
            })
        {
            return Err("wrappers must have unique indices and actual stride-24 offsets".into());
        }
        if let Some(path) = &draft.tracking_path {
            if draft.wrappers.last().is_some_and(|wrapper| {
                wrapper
                    .byte_offset
                    .and_then(|offset| offset.checked_add(24))
                    != path.wrapper_byte_offset()
            }) || draft.following_record_index != path.following_record_index()
                || Some(draft.following_byte_offset) != path.following_byte_offset()
                || draft.following_class_tag != path.following_class_tag
            {
                return Err("tracking_path disagrees with following record or wrappers".into());
            }
        } else if draft.wrappers.last().is_some_and(|wrapper| {
            wrapper
                .byte_offset
                .and_then(|offset| offset.checked_add(24))
                != Some(draft.following_byte_offset)
        }) {
            return Err("following_byte_offset must follow wrappers by 24 bytes".into());
        }
        if draft
            .persistent_identity
            .as_ref()
            .is_some_and(|persistent| {
                draft.following_byte_offset.checked_add(21) != persistent.local_id_offset()
            })
        {
            return Err(
                "persistent_identity.local_id_offset disagrees with following_byte_offset".into(),
            );
        }
        Ok(Self {
            id: draft.id,
            ordinal: draft.ordinal,
            group_record_index: draft.group_record_index,
            wrappers: draft.wrappers,
            following_record_index: draft.following_record_index,
            following_byte_offset: Some(draft.following_byte_offset),
            following_class_tag: draft.following_class_tag,
            tracking_path: draft.tracking_path,
            persistent_identity: draft.persistent_identity,
        })
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignConstructionOperandIdentityDraft, String> {
        Ok(DesignConstructionOperandIdentityDraft {
            id: self.id,
            ordinal: self.ordinal,
            group_record_index: self.group_record_index,
            wrappers: self.wrappers,
            following_record_index: self.following_record_index,
            following_byte_offset: self
                .following_byte_offset
                .ok_or("document identity has no source following_byte_offset")?,
            following_class_tag: self.following_class_tag,
            tracking_path: self.tracking_path,
            persistent_identity: self.persistent_identity,
        })
    }
    pub(crate) fn wrappers(&self) -> &Vec<DesignIdentityWrapper> {
        &self.wrappers
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.following_record_index
    }
    pub(crate) fn following_byte_offset(&self) -> Option<u64> {
        self.following_byte_offset
    }
    pub(crate) fn following_class_tag(&self) -> &DesignClassTag {
        &self.following_class_tag
    }
    pub(crate) fn tracking_path(&self) -> Option<&DesignConstructionTrackingPath> {
        self.tracking_path.as_ref()
    }
    pub(crate) fn persistent_identity(&self) -> Option<&DesignConstructionPersistentIdentity> {
        self.persistent_identity.as_ref()
    }
}
/// Actual scanner input; its source positions never enter document wire.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignConstructionOperandIdentityDraft {
    pub(crate) id: String,
    /// Semantic construction order within the containing Design stream.
    pub(crate) ordinal: u32,
    pub(crate) group_record_index: u32,
    pub(crate) wrappers: Vec<DesignIdentityWrapper>,
    pub(crate) following_record_index: u32,
    pub(crate) following_byte_offset: u64,
    pub(crate) following_class_tag: DesignClassTag,
    pub(crate) tracking_path: Option<DesignConstructionTrackingPath>,
    pub(crate) persistent_identity: Option<DesignConstructionPersistentIdentity>,
}
/// Indexed wrapper identity with an optional actual source location.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignIdentityWrapper {
    pub(crate) record_index: u32,
    pub(crate) byte_offset: Option<u64>,
    pub(crate) class_tag: DesignClassTag,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionOperandIdentityWire {
    pub(crate) id: String,
    /// Semantic construction order within the containing Design stream.
    pub(crate) ordinal: u32,
    pub(crate) group_record_index: u32,
    pub(crate) wrapper_record_indices: Vec<u32>,
    pub(crate) wrapper_class_tags: Vec<String>,
    pub(crate) following_record_index: u32,
    pub(crate) following_class_tag: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tracking_path"
    )]
    pub(crate) tracking_path: Option<DesignConstructionTrackingPath>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_persistent_identity"
    )]
    pub(crate) persistent_identity: Option<DesignConstructionPersistentIdentity>,
}
impl TryFrom<DesignConstructionOperandIdentityWire> for DesignConstructionOperandIdentity {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandIdentityWire) -> Result<Self, String> {
        if wire.wrapper_record_indices.len() != wire.wrapper_class_tags.len() {
            return Err(
                "wrapper_record_indices and wrapper_class_tags must have equal lengths".into(),
            );
        }
        distinct_construction_records("wrappers", wire.wrapper_record_indices.iter().copied())?;
        let following_class_tag: DesignClassTag = wire.following_class_tag.try_into()?;
        if wire.tracking_path.as_ref().is_some_and(|path| {
            path.following_record_index() != wire.following_record_index
                || path.following_class_tag != following_class_tag
        }) {
            return Err("tracking_path disagrees with following record identity or class".into());
        }
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            group_record_index: wire.group_record_index,
            wrappers: wire
                .wrapper_record_indices
                .into_iter()
                .zip(wire.wrapper_class_tags)
                .map(|(record_index, class_tag)| {
                    Ok(DesignIdentityWrapper {
                        record_index,
                        byte_offset: None,
                        class_tag: class_tag
                            .try_into()
                            .map_err(|error| format!("wrapper_class_tags: {error}"))?,
                    })
                })
                .collect::<Result<_, String>>()?,
            following_record_index: wire.following_record_index,
            following_byte_offset: None,
            following_class_tag,
            tracking_path: wire.tracking_path,
            persistent_identity: wire.persistent_identity,
        })
    }
}
#[cfg(test)]
impl From<DesignConstructionOperandIdentity> for DesignConstructionOperandIdentityWire {
    fn from(identity: DesignConstructionOperandIdentity) -> Self {
        Self {
            id: identity.id,
            ordinal: identity.ordinal,
            group_record_index: identity.group_record_index,
            wrapper_record_indices: identity
                .wrappers
                .iter()
                .map(|wrapper| wrapper.record_index)
                .collect(),
            wrapper_class_tags: identity
                .wrappers
                .into_iter()
                .map(|wrapper| wrapper.class_tag.into())
                .collect(),
            following_record_index: identity.following_record_index,
            following_class_tag: identity.following_class_tag.into(),
            tracking_path: identity.tracking_path,
            persistent_identity: identity.persistent_identity,
        }
    }
}

/// Entity-tracking path embedded in a construction-operand identity chain.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionTrackingPathWire")]
pub(crate) struct DesignConstructionTrackingPath {
    wrapper_record_index: u32,
    frame: Option<crate::records::frame_chain::RecordFrameChain>,
    pub(crate) wrapper_class_tag: DesignClassTag,
    pub(crate) carrier_class_tag: DesignClassTag,
    pub(crate) primary_identity: u64,
    /// One-based fragment ordinal in the serialized primary curve direction.
    pub(crate) selector: i32,
    /// Positive fragment count defined by the serialized tracking graph.
    pub(crate) kind: u32,
    first_related_identity: Option<u64>,
    second_related_identity: Option<u64>,
    pub(crate) following_class_tag: DesignClassTag,
}
#[cfg(test)]
thread_local! {static CONSTRUCTION_TRACKING_PATH_CLONE_COUNT:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
impl Clone for DesignConstructionTrackingPath {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_TRACKING_PATH_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            wrapper_record_index: self.wrapper_record_index,
            frame: self.frame,
            wrapper_class_tag: self.wrapper_class_tag.clone(),
            carrier_class_tag: self.carrier_class_tag.clone(),
            primary_identity: self.primary_identity,
            selector: self.selector,
            kind: self.kind,
            first_related_identity: self.first_related_identity,
            second_related_identity: self.second_related_identity,
            following_class_tag: self.following_class_tag.clone(),
        }
    }
}
impl Serialize for DesignConstructionTrackingPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            wrapper_record_index: u32,
            wrapper_class_tag: &'a str,
            carrier_record_index: u32,
            carrier_class_tag: &'a str,
            primary_identity: u64,
            selector: i32,
            kind: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            first_related_identity: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            second_related_identity: Option<u64>,
            following_record_index: u32,
            following_class_tag: &'a str,
        }
        WireRef {
            wrapper_record_index: self.wrapper_record_index(),
            wrapper_class_tag: self.wrapper_class_tag.as_str(),
            carrier_record_index: self.carrier_record_index(),
            carrier_class_tag: self.carrier_class_tag.as_str(),
            primary_identity: self.primary_identity,
            selector: self.selector,
            kind: self.kind,
            first_related_identity: self.first_related_identity,
            second_related_identity: self.second_related_identity,
            following_record_index: self.following_record_index(),
            following_class_tag: self.following_class_tag.as_str(),
        }
        .serialize(serializer)
    }
}
impl DesignConstructionTrackingPath {
    pub(crate) fn try_new(draft: DesignConstructionTrackingPathDraft) -> Result<Self, String> {
        if draft.kind == 0 {
            return Err("tracking kind must be a positive fragment count".into());
        }
        if !u32::try_from(draft.selector).is_ok_and(|ordinal| (1..=draft.kind).contains(&ordinal)) {
            return Err(
                "tracking selector must be a fragment ordinal from one through kind".into(),
            );
        }
        if draft.first_related_identity.is_some_and(|identity| {
            draft.wrapper_byte_offset.checked_add(110) != Some(identity.offset)
        }) || draft.second_related_identity.is_some_and(|identity| {
            draft
                .wrapper_byte_offset
                .checked_add(114 + u64::from(draft.first_related_identity.is_some()) * 8)
                != Some(identity.offset)
        }) {
            return Err("related_identity_offset disagrees with tracking frame".into());
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.wrapper_record_index,
            draft.wrapper_byte_offset,
            2,
            114 + u64::from(draft.first_related_identity.is_some()) * 8
                + u64::from(draft.second_related_identity.is_some()) * 8,
        )?;
        let value = Self {
            wrapper_record_index: draft.wrapper_record_index,
            frame: Some(frame),
            wrapper_class_tag: draft.wrapper_class_tag,
            carrier_class_tag: draft.carrier_class_tag,
            primary_identity: draft.primary_identity,
            selector: draft.selector,
            kind: draft.kind,
            first_related_identity: draft.first_related_identity.map(|value| value.value),
            second_related_identity: draft.second_related_identity.map(|value| value.value),
            following_class_tag: draft.following_class_tag,
        };
        if value.carrier_record_index() != draft.carrier_record_index {
            return Err("carrier_record_index disagrees with frame layout".into());
        }
        if value.carrier_byte_offset() != Some(draft.carrier_byte_offset) {
            return Err("carrier_byte_offset disagrees with frame layout".into());
        }
        if value.primary_identity_offset() != Some(draft.primary_identity_offset) {
            return Err("primary_identity_offset disagrees with frame layout".into());
        }
        if value.selector_offset() != Some(draft.selector_offset) {
            return Err("selector_offset disagrees with frame layout".into());
        }
        if value.kind_offset() != Some(draft.kind_offset) {
            return Err("kind_offset disagrees with frame layout".into());
        }
        if value.following_record_index() != draft.following_record_index {
            return Err("following_record_index disagrees with frame layout".into());
        }
        if value.following_byte_offset() != Some(draft.following_byte_offset) {
            return Err("following_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignConstructionTrackingPathDraft, String> {
        let first_related_identity = self
            .first_related_identity()
            .map(|value| {
                Ok::<_, String>(Located {
                    value: value.value,
                    offset: value
                        .offset
                        .ok_or("document tracking identity has no source related offset")?,
                })
            })
            .transpose()?;
        let second_related_identity = self
            .second_related_identity()
            .map(|value| {
                Ok::<_, String>(Located {
                    value: value.value,
                    offset: value
                        .offset
                        .ok_or("document tracking identity has no source related offset")?,
                })
            })
            .transpose()?;
        Ok(DesignConstructionTrackingPathDraft {
            wrapper_record_index: self.wrapper_record_index(),
            wrapper_byte_offset: self
                .wrapper_byte_offset()
                .ok_or("document tracking path has no source wrapper offset")?,
            carrier_record_index: self.carrier_record_index(),
            carrier_byte_offset: self
                .carrier_byte_offset()
                .ok_or("document tracking path has no source carrier offset")?,
            primary_identity_offset: self
                .primary_identity_offset()
                .ok_or("document tracking path has no source identity offset")?,
            selector_offset: self
                .selector_offset()
                .ok_or("document tracking path has no source selector offset")?,
            kind_offset: self
                .kind_offset()
                .ok_or("document tracking path has no source kind offset")?,
            following_record_index: self.following_record_index(),
            following_byte_offset: self
                .following_byte_offset()
                .ok_or("document tracking path has no source following offset")?,
            wrapper_class_tag: self.wrapper_class_tag,
            carrier_class_tag: self.carrier_class_tag,
            primary_identity: self.primary_identity,
            selector: self.selector,
            kind: self.kind,
            first_related_identity,
            second_related_identity,
            following_class_tag: self.following_class_tag,
        })
    }
    pub(crate) fn wrapper_record_index(&self) -> u32 {
        self.wrapper_record_index
    }
    pub(crate) fn wrapper_byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(0))
    }
    pub(crate) fn carrier_record_index(&self) -> u32 {
        self.wrapper_record_index + 1
    }
    pub(crate) fn carrier_byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(33))
    }
    pub(crate) fn primary_identity_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(70))
    }
    fn selector_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(90))
    }
    fn kind_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(94))
    }
    #[cfg(test)]
    pub(crate) fn first_related_identity(&self) -> Option<Located<u64, Option<u64>>> {
        self.first_related_identity.map(|value| Located {
            value,
            offset: self.frame.map(|frame| frame.offset(110)),
        })
    }
    #[cfg(test)]
    pub(crate) fn second_related_identity(&self) -> Option<Located<u64, Option<u64>>> {
        self.second_related_identity.map(|value| Located {
            value,
            offset: self.frame.map(|frame| {
                frame.offset(114 + u64::from(self.first_related_identity.is_some()) * 8)
            }),
        })
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.wrapper_record_index + 2
    }
    pub(crate) fn following_byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| {
            frame.offset(
                114 + u64::from(self.first_related_identity.is_some()) * 8
                    + u64::from(self.second_related_identity.is_some()) * 8,
            )
        })
    }
}
/// Actual source tracking path, admitted independently of document content.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignConstructionTrackingPathDraft {
    pub(crate) wrapper_record_index: u32,
    pub(crate) wrapper_byte_offset: u64,
    pub(crate) wrapper_class_tag: DesignClassTag,
    pub(crate) carrier_record_index: u32,
    pub(crate) carrier_byte_offset: u64,
    pub(crate) carrier_class_tag: DesignClassTag,
    pub(crate) primary_identity: u64,
    pub(crate) primary_identity_offset: u64,
    /// One-based fragment ordinal in the serialized primary curve direction.
    pub(crate) selector: i32,
    pub(crate) selector_offset: u64,
    /// Positive fragment count defined by the serialized tracking graph.
    pub(crate) kind: u32,
    pub(crate) kind_offset: u64,
    pub(crate) first_related_identity: Option<Located<u64>>,
    pub(crate) second_related_identity: Option<Located<u64>>,
    pub(crate) following_record_index: u32,
    pub(crate) following_byte_offset: u64,
    pub(crate) following_class_tag: DesignClassTag,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionTrackingPathWire {
    pub(crate) wrapper_record_index: u32,
    pub(crate) wrapper_class_tag: String,
    pub(crate) carrier_record_index: u32,
    pub(crate) carrier_class_tag: String,
    pub(crate) primary_identity: u64,
    pub(crate) selector: i32,
    pub(crate) kind: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_related_identity"
    )]
    pub(crate) first_related_identity: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_related_identity"
    )]
    pub(crate) second_related_identity: Option<u64>,
    pub(crate) following_record_index: u32,
    pub(crate) following_class_tag: String,
}
impl TryFrom<DesignConstructionTrackingPathWire> for DesignConstructionTrackingPath {
    type Error = String;
    fn try_from(wire: DesignConstructionTrackingPathWire) -> Result<Self, String> {
        if wire.wrapper_record_index.checked_add(1) != Some(wire.carrier_record_index) {
            return Err("carrier_record_index disagrees with tracking record chain".into());
        }
        if wire.wrapper_record_index.checked_add(2) != Some(wire.following_record_index) {
            return Err("following_record_index disagrees with tracking record chain".into());
        }
        if wire.kind == 0 {
            return Err("tracking kind must be a positive fragment count".into());
        }
        if !u32::try_from(wire.selector).is_ok_and(|ordinal| (1..=wire.kind).contains(&ordinal)) {
            return Err(
                "tracking selector must be a fragment ordinal from one through kind".into(),
            );
        }
        Ok(Self {
            wrapper_record_index: wire.wrapper_record_index,
            frame: None,
            wrapper_class_tag: wire.wrapper_class_tag.try_into()?,
            carrier_class_tag: wire.carrier_class_tag.try_into()?,
            primary_identity: wire.primary_identity,
            selector: wire.selector,
            kind: wire.kind,
            first_related_identity: wire.first_related_identity,
            second_related_identity: wire.second_related_identity,
            following_class_tag: wire.following_class_tag.try_into()?,
        })
    }
}
#[cfg(test)]
impl From<DesignConstructionTrackingPath> for DesignConstructionTrackingPathWire {
    fn from(value: DesignConstructionTrackingPath) -> Self {
        Self {
            wrapper_record_index: value.wrapper_record_index(),
            carrier_record_index: value.carrier_record_index(),
            following_record_index: value.following_record_index(),
            wrapper_class_tag: value.wrapper_class_tag.into(),
            carrier_class_tag: value.carrier_class_tag.into(),
            primary_identity: value.primary_identity,
            selector: value.selector,
            kind: value.kind,
            first_related_identity: value.first_related_identity,
            second_related_identity: value.second_related_identity,
            following_class_tag: value.following_class_tag.into(),
        }
    }
}

/// Fixed-width persistent identity following a construction-operand identity chain.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionPersistentIdentityContent")]
pub(crate) struct DesignConstructionPersistentIdentity {
    source: Option<PersistentIdentitySource>,
    pub(crate) local_id: u64,
    pub(crate) asset_id: DesignRelaxedGuidText,
    pub(crate) context_id: DesignRelaxedGuidText,
    pub(crate) next_record_index: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PersistentIdentitySource {
    tail: PersistentIdentityTail,
    local_id_offset: u64,
    context_id_offset: u64,
    tail_slot_present: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistentIdentityTail {
    Fixed { tail_slot_offset: u64 },
    Extended,
}
#[cfg(test)]
thread_local! {pub(super) static PERSISTENT_IDENTITY_CLONE_COUNT:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
impl Clone for DesignConstructionPersistentIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        PERSISTENT_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            source: self.source,
            local_id: self.local_id,
            asset_id: self.asset_id.clone(),
            context_id: self.context_id.clone(),
            next_record_index: self.next_record_index,
        }
    }
}
impl Serialize for DesignConstructionPersistentIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            local_id: u64,
            asset_id: &'a str,
            context_id: &'a str,
            next_record_index: u32,
        }
        WireRef {
            local_id: self.local_id,
            asset_id: self.asset_id.as_str(),
            context_id: self.context_id.as_str(),
            next_record_index: self.next_record_index,
        }
        .serialize(serializer)
    }
}
impl DesignConstructionPersistentIdentity {
    pub(crate) fn try_new(
        draft: DesignConstructionPersistentIdentityDraft,
    ) -> Result<Self, String> {
        let base = draft
            .local_id_offset
            .checked_sub(21)
            .ok_or("local_id_offset precedes identity header")?;
        if draft.local_id_offset.checked_add(12) != Some(draft.asset_id_offset)
            || draft.context_id_offset <= draft.asset_id_offset
            || !(base.checked_add(190) == Some(draft.next_byte_offset)
                || (base.checked_add(185) == Some(draft.tail_slot_offset)
                    && draft.tail_slot_offset.checked_add(15) == Some(draft.next_byte_offset)))
        {
            return Err("persistent identity offsets disagree with frame layout".into());
        }
        let tail = if base.checked_add(190) == Some(draft.next_byte_offset) {
            PersistentIdentityTail::Fixed {
                tail_slot_offset: draft.tail_slot_offset,
            }
        } else {
            PersistentIdentityTail::Extended
        };
        Ok(Self {
            source: Some(PersistentIdentitySource {
                tail,
                local_id_offset: draft.local_id_offset,
                context_id_offset: draft.context_id_offset,
                tail_slot_present: draft.tail_slot_present,
            }),
            local_id: draft.local_id,
            asset_id: draft.asset_id,
            context_id: draft.context_id,
            next_record_index: draft.next_record_index,
        })
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignConstructionPersistentIdentityDraft, String> {
        let source = self
            .source
            .ok_or("document persistent identity has no source layout")?;
        Ok(DesignConstructionPersistentIdentityDraft {
            local_id: self.local_id,
            local_id_offset: source.local_id_offset,
            asset_id_offset: source.local_id_offset + 12,
            context_id_offset: source.context_id_offset,
            tail_slot_present: source.tail_slot_present,
            tail_slot_offset: match source.tail {
                PersistentIdentityTail::Fixed { tail_slot_offset } => tail_slot_offset,
                PersistentIdentityTail::Extended => source.local_id_offset + 164,
            },
            next_byte_offset: source.local_id_offset
                + match source.tail {
                    PersistentIdentityTail::Fixed { .. } => 169,
                    PersistentIdentityTail::Extended => 179,
                },
            asset_id: self.asset_id,
            context_id: self.context_id,
            next_record_index: self.next_record_index,
        })
    }
    pub(crate) fn local_id_offset(&self) -> Option<u64> {
        self.source.map(|source| source.local_id_offset)
    }
    pub(crate) fn asset_id_offset(&self) -> Option<u64> {
        self.source.map(|source| source.local_id_offset + 12)
    }
    pub(crate) fn context_id_offset(&self) -> Option<u64> {
        self.source.map(|source| source.context_id_offset)
    }
    pub(crate) fn tail_slot_offset(&self) -> Option<u64> {
        self.source.map(|source| match source.tail {
            PersistentIdentityTail::Fixed { tail_slot_offset } => tail_slot_offset,
            PersistentIdentityTail::Extended => source.local_id_offset + 164,
        })
    }
    pub(crate) fn next_byte_offset(&self) -> Option<u64> {
        self.source.map(|source| {
            source.local_id_offset
                + match source.tail {
                    PersistentIdentityTail::Fixed { .. } => 169,
                    PersistentIdentityTail::Extended => 179,
                }
        })
    }
}
/// Source-only persistent identity scanner input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignConstructionPersistentIdentityDraft {
    pub(crate) local_id: u64,
    pub(crate) local_id_offset: u64,
    pub(crate) asset_id: DesignRelaxedGuidText,
    pub(crate) asset_id_offset: u64,
    pub(crate) context_id: DesignRelaxedGuidText,
    pub(crate) context_id_offset: u64,
    #[serde(default)]
    pub(crate) tail_slot_present: bool,
    #[serde(default)]
    pub(crate) tail_slot_offset: u64,
    pub(crate) next_record_index: u32,
    pub(crate) next_byte_offset: u64,
}
impl TryFrom<DesignConstructionPersistentIdentityDraft> for DesignConstructionPersistentIdentity {
    type Error = String;
    fn try_from(draft: DesignConstructionPersistentIdentityDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConstructionPersistentIdentityContent {
    pub(crate) local_id: u64,
    pub(crate) asset_id: DesignRelaxedGuidText,
    pub(crate) context_id: DesignRelaxedGuidText,
    pub(crate) next_record_index: u32,
}
impl TryFrom<DesignConstructionPersistentIdentityContent> for DesignConstructionPersistentIdentity {
    type Error = String;
    fn try_from(content: DesignConstructionPersistentIdentityContent) -> Result<Self, String> {
        Ok(Self {
            source: None,
            local_id: content.local_id,
            asset_id: content.asset_id,
            context_id: content.context_id,
            next_record_index: content.next_record_index,
        })
    }
}
#[cfg(test)]
impl From<DesignConstructionPersistentIdentity> for DesignConstructionPersistentIdentityContent {
    fn from(value: DesignConstructionPersistentIdentity) -> Self {
        Self {
            local_id: value.local_id,
            asset_id: value.asset_id,
            context_id: value.context_id,
            next_record_index: value.next_record_index,
        }
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_extrude_role,
    DesignExtrudeOperandRoleTag,
    "extrude_role"
);

cadmpeg_core::named_optional_field!(
    deserialize_extrude_face_role,
    DesignExtrudeFaceRole,
    "extrude_face_role"
);

cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");

cadmpeg_core::named_optional_field!(deserialize_compact_variant, bool, "compact_variant");

cadmpeg_core::named_optional_field!(
    deserialize_tracking_path,
    DesignConstructionTrackingPath,
    "tracking_path"
);

cadmpeg_core::named_optional_field!(
    deserialize_persistent_identity,
    DesignConstructionPersistentIdentity,
    "persistent_identity"
);

cadmpeg_core::named_optional_field!(
    deserialize_first_related_identity,
    u64,
    "first_related_identity"
);

cadmpeg_core::named_optional_field!(
    deserialize_second_related_identity,
    u64,
    "second_related_identity"
);

#[cfg(test)]
mod tests;
