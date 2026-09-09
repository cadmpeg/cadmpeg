// SPDX-License-Identifier: Apache-2.0
//! Historical topology selections, recipe operands, and incidence records.

use super::feature::DesignAxis;
use super::SketchPlacementMatrix;
use super::{
    ConstructionRecipeKind, DesignRecipeReference, Located, NonEmptyByteSpan, SketchRelationOperand,
};
use super::{DesignClassTag, DesignEntityId, DesignRelaxedGuidText, DesignSecondaryIdentity};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;

/// Sketch-profile selection frame named by a profile-based feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchProfileOperandWire",
    into = "DesignSketchProfileOperandWire"
)]
pub struct DesignSketchProfileOperand {
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the selected Sketch reference.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    pub entity_id: DesignEntityId,
    /// Byte offset of the suffix's UTF-16LE code units.
    pub entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignSketchProfileOperandWire {
    /// Zero-based position in the scope's ordered reference table.
    scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selected Sketch reference.
    asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    entity_id: String,
    /// Numeric suffix stored by the profile frame.
    entity_suffix: u64,
    /// Byte offset of the suffix's UTF-16LE code units.
    entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
}

impl TryFrom<DesignSketchProfileOperandWire> for DesignSketchProfileOperand {
    type Error = String;

    fn try_from(wire: DesignSketchProfileOperandWire) -> Result<Self, Self::Error> {
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        Ok(Self {
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            entity_id,
            entity_reference_offset: wire.entity_reference_offset,
            region_selection: wire.region_selection,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignSketchProfileOperand> for DesignSketchProfileOperandWire {
    fn from(value: DesignSketchProfileOperand) -> Self {
        let entity_suffix = value.entity_id.suffix();
        Self {
            scope_reference_ordinal: value.scope_reference_ordinal,
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag.into(),
            asset_id: value.asset_id.into(),
            asset_id_offset: value.asset_id_offset,
            entity_id: value.entity_id.0,
            entity_suffix,
            entity_reference_offset: value.entity_reference_offset,
            region_selection: value.region_selection,
            paired_class_tag: value.paired_class_tag.into(),
            paired_byte_offset: value.paired_byte_offset,
        }
    }
}

/// Nested ordered region selection carried by a sketch-profile operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSketchProfileRegionSelection {
    /// Indexed identity of the region-selection record.
    pub record_index: u32,
    /// Byte offset of the region-selection indexed header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII region-selection class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the selected-region count.
    pub region_count_offset: u64,
    /// Selected regions in source order.
    pub regions: Vec<DesignSketchProfileRegion>,
    /// Source per-file dynamic three-digit ASCII companion class tag.
    pub companion_class_tag: DesignClassTag,
    /// Byte offset of the same-index companion header.
    pub companion_byte_offset: u64,
}

/// One selected region in a nested sketch-profile selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignSketchProfileRegion {
    /// Byte offset of this region's member count.
    pub member_count_offset: u64,
    /// Persistent curve members in source order.
    pub members: Vec<DesignSketchProfileRegionMember>,
}

/// One fixed-width persistent curve member of a selected sketch region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchProfileRegionMemberWire",
    into = "DesignSketchProfileRegionMemberWire"
)]
pub struct DesignSketchProfileRegionMember {
    /// Byte offset of the member-kind code.
    pub kind_offset: u64,
    /// Primary persistent identity of the selected Sketch curve.
    pub curve_primary_id: std::num::NonZeroU32,
    /// Byte offset of the persistent curve identity.
    pub curve_primary_id_offset: u64,
    /// First incidence value, encoded as zero or one.
    pub incidence_flag: bool,
    /// Second and third incidence values, in source order.
    pub incidence_values: [DesignRegionIncidence; 2],
    /// Byte offset of the first incidence word.
    pub incidence_words_offset: u64,
}

/// One of the two admitted nonzero region-incidence values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignRegionIncidence {
    One,
    Two,
}

impl TryFrom<u32> for DesignRegionIncidence {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            _ => Err("incidence_words second and third values must be 1 or 2".into()),
        }
    }
}

impl From<DesignRegionIncidence> for u32 {
    fn from(value: DesignRegionIncidence) -> Self {
        match value {
            DesignRegionIncidence::One => 1,
            DesignRegionIncidence::Two => 2,
        }
    }
}

/// One fixed-width persistent curve member of a selected sketch region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignSketchProfileRegionMemberWire {
    /// Native member-kind code. Profile-region curve members use value three.
    kind: u32,
    /// Byte offset of the member-kind code.
    kind_offset: u64,
    /// Primary persistent identity of the selected Sketch curve.
    curve_primary_id: u64,
    /// Byte offset of the persistent curve identity.
    curve_primary_id_offset: u64,
    /// Structural region-incidence words retained in source order.
    incidence_words: [u32; 8],
    /// Byte offset of the first incidence word.
    incidence_words_offset: u64,
}

impl TryFrom<DesignSketchProfileRegionMemberWire> for DesignSketchProfileRegionMember {
    type Error = String;
    fn try_from(wire: DesignSketchProfileRegionMemberWire) -> Result<Self, Self::Error> {
        if wire.kind != 3 {
            return Err("kind must be 3 for a profile-region curve member".into());
        }
        let curve_primary_id = u32::try_from(wire.curve_primary_id)
            .ok()
            .and_then(std::num::NonZeroU32::new)
            .ok_or("curve_primary_id must be a nonzero u32")?;
        let [0, 0, 0, flag, first, second, 0, 0] = wire.incidence_words else {
            return Err("incidence_words must contain three leading and two trailing zeros".into());
        };
        let incidence_flag = match flag {
            0 => false,
            1 => true,
            _ => return Err("incidence_words first value must be 0 or 1".into()),
        };
        Ok(Self {
            kind_offset: wire.kind_offset,
            curve_primary_id,
            curve_primary_id_offset: wire.curve_primary_id_offset,
            incidence_flag,
            incidence_values: [
                DesignRegionIncidence::try_from(first)?,
                DesignRegionIncidence::try_from(second)?,
            ],
            incidence_words_offset: wire.incidence_words_offset,
        })
    }
}

impl From<DesignSketchProfileRegionMember> for DesignSketchProfileRegionMemberWire {
    fn from(member: DesignSketchProfileRegionMember) -> Self {
        Self {
            kind: 3,
            kind_offset: member.kind_offset,
            curve_primary_id: u64::from(member.curve_primary_id.get()),
            curve_primary_id_offset: member.curve_primary_id_offset,
            incidence_words: [
                0,
                0,
                0,
                u32::from(member.incidence_flag),
                member.incidence_values[0].into(),
                member.incidence_values[1].into(),
                0,
                0,
            ],
            incidence_words_offset: member.incidence_words_offset,
        }
    }
}

/// Counted selection group owned by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignExtrudeSelectionGroupWire",
    into = "DesignExtrudeSelectionGroupWire"
)]
pub struct DesignExtrudeSelectionGroup {
    /// Globally unique deterministic identifier for this native group.
    pub id: String,
    /// Owning Extrude parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Ordered indexed selection-member records.
    members: Vec<Located<u32>>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    pub opaque_index: NonZeroU32,
    /// Opaque finite f64 between the repeated u32 copies.
    opaque_scalar: f64,
    /// Boolean byte between the two nested-record references.
    pub variant: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
}

/// Counted selection group owned by an Extrude parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignExtrudeSelectionGroupWire {
    /// Globally unique deterministic identifier for this native group.
    pub id: String,
    /// Owning Extrude parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: String,
    /// Byte offset of the counted member-run length.
    pub member_count_offset: u64,
    /// Ordered indexed selection-member records.
    pub members: Vec<u32>,
    /// Byte offsets parallel to `members`.
    pub member_offsets: Vec<u64>,
    /// Opaque nonzero u32 repeated around the f64 scalar.
    pub opaque_index: u32,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque finite f64 between the repeated u32 copies.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Boolean byte between the two nested-record references.
    pub variant: bool,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: String,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
}

impl TryFrom<DesignExtrudeSelectionGroupWire> for DesignExtrudeSelectionGroup {
    type Error = String;
    fn try_from(wire: DesignExtrudeSelectionGroupWire) -> Result<Self, Self::Error> {
        if wire.members.len() != wire.member_offsets.len() {
            return Err("members and member_offsets must have equal lengths".into());
        }
        let offsets = DesignExtrudeSelectionGroup::offsets(wire.byte_offset, wire.members.len())?;
        if wire
            .members
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != wire.members.len()
        {
            return Err("members must be distinct".into());
        }
        if wire.member_count_offset != offsets[0]
            || wire.opaque_index_offset != offsets[1]
            || wire.opaque_scalar_offset != offsets[2]
            || wire.paired_byte_offset != offsets[3]
        {
            return Err("member_count_offset, opaque_index_offset, opaque_scalar_offset and paired_byte_offset must follow the member run".into());
        }
        if !wire
            .member_offsets
            .iter()
            .enumerate()
            .all(|(index, offset)| *offset == offsets[0] + 5 + index as u64 * 11)
        {
            return Err(
                "member_offsets must start after member_count_offset and have stride 11".into(),
            );
        }
        let opaque_index =
            NonZeroU32::new(wire.opaque_index).ok_or("opaque_index must be nonzero")?;
        if !wire.opaque_scalar.is_finite() {
            return Err("opaque_scalar must be finite".into());
        }
        Ok(Self {
            members: wire
                .members
                .into_iter()
                .zip(wire.member_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            opaque_index,
            opaque_scalar: wire.opaque_scalar,
            variant: wire.variant,
            paired_class_tag: wire.paired_class_tag.try_into()?,
        })
    }
}

impl From<DesignExtrudeSelectionGroup> for DesignExtrudeSelectionGroupWire {
    fn from(group: DesignExtrudeSelectionGroup) -> Self {
        let opaque_scalar = group.opaque_scalar();
        let member_count_offset = group.member_count_offset();
        let opaque_index_offset = group.opaque_index_offset();
        let opaque_scalar_offset = group.opaque_scalar_offset();
        let paired_byte_offset = group.paired_byte_offset();
        let (members, member_offsets) = group
            .members
            .into_iter()
            .map(|member| (member.value, member.offset))
            .unzip();
        Self {
            members,
            member_offsets,
            id: group.id,
            scope_record_index: group.scope_record_index,
            scope_reference_ordinal: group.scope_reference_ordinal,
            record_index: group.record_index,
            byte_offset: group.byte_offset,
            class_tag: group.class_tag.into(),
            member_count_offset,
            opaque_index: group.opaque_index.get(),
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset,
            variant: group.variant,
            paired_class_tag: group.paired_class_tag.into(),
            paired_byte_offset,
        }
    }
}

impl DesignExtrudeSelectionGroup {
    fn offsets(byte_offset: u64, count: usize) -> Result<[u64; 4], String> {
        if count == 0 {
            return Err("members must not be empty".into());
        }
        let member_count_offset = byte_offset
            .checked_add(32)
            .ok_or("byte_offset overflows member_count_offset")?;
        let run_size = u64::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(11))
            .ok_or("members exceed offset range")?;
        let opaque_index_offset = member_count_offset
            .checked_add(4)
            .and_then(|offset| offset.checked_add(run_size))
            .ok_or("members overflow opaque_index_offset")?;
        let paired_byte_offset = opaque_index_offset
            .checked_add(53)
            .ok_or("opaque_index_offset overflows paired_byte_offset")?;
        Ok([
            member_count_offset,
            opaque_index_offset,
            opaque_index_offset + 4,
            paired_byte_offset,
        ])
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn members(&self) -> &[Located<u32>] {
        &self.members
    }
    pub(crate) fn member_count_offset(&self) -> u64 {
        self.byte_offset + 32
    }
    pub(crate) fn opaque_index_offset(&self) -> u64 {
        self.byte_offset + 36 + self.members.len() as u64 * 11
    }
    pub(crate) fn opaque_scalar_offset(&self) -> u64 {
        self.opaque_index_offset() + 4
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.opaque_index_offset() + 53
    }
    pub(crate) fn opaque_scalar(&self) -> f64 {
        self.opaque_scalar
    }
    #[cfg(test)]
    pub(crate) fn try_set_members(&mut self, members: Vec<u32>) -> Result<(), String> {
        let offsets = Self::offsets(self.byte_offset, members.len())?;
        let mut wire = DesignExtrudeSelectionGroupWire::from(self.clone());
        wire.member_offsets = (0..members.len())
            .map(|index| offsets[0] + 5 + index as u64 * 11)
            .collect();
        wire.members = members;
        [
            wire.member_count_offset,
            wire.opaque_index_offset,
            wire.opaque_scalar_offset,
            wire.paired_byte_offset,
        ] = offsets;
        *self = Self::try_from(wire)?;
        Ok(())
    }
}

/// Semantic role of a counted Extrude operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignExtrudeOperandRole {
    /// Existing bodies consumed by the Boolean operation.
    Bodies,
    /// Sketch profile swept by the Extrude.
    Profile,
    /// Faces used by profile-start or termination construction. The ordered
    /// position inside the scope always resolves to one of the two uses, so
    /// there is no unresolved face role.
    Faces(DesignExtrudeFaceRole),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesignExtrudeOperandRoleTag {
    Bodies,
    Profile,
    Faces,
}

/// Semantic use of an ordered Extrude face-operand group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignExtrudeFaceRole {
    /// Face supporting a selected-face start.
    Start,
    /// Face terminating a one-sided to-face extent.
    Termination,
}

/// Source u64 role code carried by a construction-operand group.
///
/// The admitted set is open: files carry codes outside the named list, and
/// `extrude_operand_role` returns `None` for them, so this is a newtype with
/// named codes rather than a closed enum. One spelling per value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DesignOperandRole(u64);

impl DesignOperandRole {
    /// Extrude body operand run A.
    pub const BODIES_A: Self = Self(0x0000_0004_0000_0000);
    /// Extrude body operand run B.
    pub const BODIES_B: Self = Self(0x0000_0008_0000_0000);
    /// Extrude profile operand run.
    pub const PROFILE: Self = Self(0x0000_0041_0000_0000);
    /// Extrude face operand run.
    pub const FACES: Self = Self(0x0000_0011_0000_0000);

    // These codes have scope-dependent meanings and no single semantic name.
    /// Scope-dependent role code 0x5.
    pub const ROLE_0X5: Self = Self(0x0000_0005_0000_0000);
    /// Scope-dependent role code 0x7.
    pub const ROLE_0X7: Self = Self(0x0000_0007_0000_0000);
    /// Scope-dependent role code 0x9.
    pub const ROLE_0X9: Self = Self(0x0000_0009_0000_0000);
    /// Scope-dependent role code 0x10.
    pub const ROLE_0X10: Self = Self(0x0000_0010_0000_0000);
    /// Scope-dependent role code 0x12.
    pub const ROLE_0X12: Self = Self(0x0000_0012_0000_0000);
    /// Scope-dependent role code 0x21.
    pub const ROLE_0X21: Self = Self(0x0000_0021_0000_0000);
    /// Scope-dependent role code 0x43.
    pub const ROLE_0X43: Self = Self(0x0000_0043_0000_0000);

    /// Wrap the stored u64 role code.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    /// The stored u64 role code.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Source encoding of an Extrude face operand run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignExtrudeFaceEncoding {
    /// Standard face-group encoding.
    Faces,
    /// Selected-face start encoding.
    SelectedStart,
    /// Legacy termination-face encoding.
    LegacyTermination,
}

/// A construction role classified in its owning scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignConstructionOperandRole {
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
    pub fn source(self) -> DesignOperandRole {
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
    pub fn extrude(self) -> Option<DesignExtrudeOperandRole> {
        match self {
            Self::Other(_) => None,
            Self::ExtrudeBodiesA | Self::ExtrudeBodiesB => Some(DesignExtrudeOperandRole::Bodies),
            Self::ExtrudeProfile => Some(DesignExtrudeOperandRole::Profile),
            Self::ExtrudeFaces { usage, .. } => Some(DesignExtrudeOperandRole::Faces(usage)),
        }
    }
}

/// Construction-operand group owned by a feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignConstructionOperandGroupSerde",
    into = "DesignConstructionOperandGroupSerde"
)]
pub struct DesignConstructionOperandGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Position in the scope reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Ordered operand-record references.
    members: Vec<Located<u32>>,
    /// Ordered unresolved-edge records whose run terminates at this group's identity.
    pub lost_edge_references: Vec<String>,
    /// Exact framing of the operand-member run and its auxiliary fields.
    pub frame: DesignConstructionOperandGroupFrame,
    /// Source role classified in its owning scope.
    pub operand_role: DesignConstructionOperandRole,
    /// Per-file dynamic paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Same-index paired-header byte offset.
    pub paired_byte_offset: u64,
}

/// Unchecked construction-group input.
pub(crate) struct DesignConstructionOperandGroupDraft {
    pub id: String,
    pub scope_record_index: u32,
    pub scope_reference_ordinal: u32,
    pub record_index: u32,
    pub byte_offset: u64,
    pub class_tag: DesignClassTag,
    pub members: Vec<Located<u32>>,
    pub lost_edge_references: Vec<String>,
    pub frame: DesignConstructionOperandGroupFrame,
    pub operand_role: DesignConstructionOperandRole,
    pub role_offset: u64,
    pub paired_class_tag: DesignClassTag,
    pub paired_byte_offset: u64,
}

impl TryFrom<DesignConstructionOperandGroupDraft> for DesignConstructionOperandGroup {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandGroupDraft) -> Result<Self, Self::Error> {
        Self::check_members(&draft.members)?;
        if draft.role_offset != draft.frame.role_offset() {
            return Err("role_offset must precede opaque_index_offset by 18 bytes".into());
        }
        Ok(Self {
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            scope_reference_ordinal: draft.scope_reference_ordinal,
            record_index: draft.record_index,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            members: draft.members,
            lost_edge_references: draft.lost_edge_references,
            frame: draft.frame,
            operand_role: draft.operand_role,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: draft.paired_byte_offset,
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
    pub fn members(&self) -> &[Located<u32>] {
        &self.members
    }

    /// Checked replacement of the operand-reference run.
    pub(crate) fn try_set_members(&mut self, members: Vec<Located<u32>>) -> Result<(), String> {
        Self::check_members(&members)?;
        self.members = members;
        Ok(())
    }

    /// Role offset derived from the opaque-index location.
    pub fn role_offset(&self) -> u64 {
        self.frame.role_offset()
    }

    /// The source role code.
    pub fn role(&self) -> DesignOperandRole {
        self.operand_role.source()
    }

    /// The Extrude role derived from the scope-classified source encoding.
    pub fn extrude_role(&self) -> Option<DesignExtrudeOperandRole> {
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
struct DesignConstructionOperandGroupSerde {
    id: String,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    members: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lost_edge_references: Vec<String>,
    member_offsets: Vec<u64>,
    frame: DesignConstructionOperandGroupFrame,
    role: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extrude_role: Option<DesignExtrudeOperandRoleTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extrude_face_role: Option<DesignExtrudeFaceRole>,
    role_offset: u64,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignConstructionOperandGroupSerde> for DesignConstructionOperandGroup {
    type Error = String;

    fn try_from(wire: DesignConstructionOperandGroupSerde) -> Result<Self, Self::Error> {
        if wire.members.len() != wire.member_offsets.len() {
            return Err("members and member_offsets must have equal lengths".into());
        }
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
        Self::try_from(DesignConstructionOperandGroupDraft {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            members: wire
                .members
                .into_iter()
                .zip(wire.member_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            lost_edge_references: wire.lost_edge_references,
            frame: wire.frame,
            operand_role,
            role_offset: wire.role_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignConstructionOperandGroup> for DesignConstructionOperandGroupSerde {
    fn from(group: DesignConstructionOperandGroup) -> Self {
        let role_offset = group.role_offset();
        let (members, member_offsets) = group
            .members
            .into_iter()
            .map(|member| (member.value, member.offset))
            .unzip();
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
            byte_offset: group.byte_offset,
            class_tag: group.class_tag.into(),
            members,
            lost_edge_references: group.lost_edge_references,
            member_offsets,
            frame: group.frame,
            role: group.operand_role.source().raw(),
            extrude_role,
            extrude_face_role,
            role_offset,
            paired_class_tag: group.paired_class_tag.into(),
            paired_byte_offset: group.paired_byte_offset,
        }
    }
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignConstructionOperandGroupFrameWire",
    into = "DesignConstructionOperandGroupFrameWire"
)]
pub struct DesignConstructionOperandGroupFrame {
    /// Byte offset of the member count.
    pub member_count_offset: u64,
    /// Auxiliary records named by the two optional references that follow the
    /// member run; an absent reference contributes no entry.
    pub auxiliary_records: Vec<Located<u32>>,
    /// Exact selection-path records selected by the optional references.
    auxiliary_paths: Vec<DesignConstructionOperandPath>,
    /// Indexed records named by the counted trailing-reference run. The target
    /// grammar is selected by the owning operand family: persistent-selection
    /// groups name identity wrappers and placed-selection groups name affine
    /// transforms.
    trailing_records: Option<Located<u32>>,
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
    pub opaque_index: NonZeroU32,
    /// Byte offset of the first `opaque_index` copy.
    opaque_index_offset: u64,
    /// Opaque nonnegative finite f64.
    opaque_scalar: f64,
    /// Boolean tail variant.
    pub variant: bool,
}

/// Unchecked construction-frame input.
pub(crate) struct DesignConstructionOperandGroupFrameDraft {
    pub member_count_offset: u64,
    pub auxiliary_records: Vec<Located<u32>>,
    pub auxiliary_paths: Vec<DesignConstructionOperandPath>,
    pub trailing_records: Vec<Located<u32>>,
    pub trailing_transforms: Vec<DesignConstructionOperandTransform>,
    pub trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    pub trailing_flags: Vec<DesignConstructionOperandFlag>,
    pub opaque_index: u32,
    pub opaque_index_offset: u64,
    pub opaque_scalar: f64,
    pub opaque_scalar_offset: u64,
    pub variant: bool,
}

impl TryFrom<DesignConstructionOperandGroupFrameDraft> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandGroupFrameDraft) -> Result<Self, Self::Error> {
        if draft.trailing_records.len() > 1 {
            return Err("construction frame permits at most one trailing record".into());
        }
        let opaque_index =
            NonZeroU32::new(draft.opaque_index).ok_or("opaque_index must be nonzero")?;
        if !draft.opaque_scalar.is_finite() || draft.opaque_scalar < 0.0 {
            return Err("opaque_scalar must be finite and nonnegative".into());
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
                .map(|record| record.record_index),
        )?;
        distinct_construction_records(
            "trailing_transforms",
            draft
                .trailing_transforms
                .iter()
                .map(|record| record.record_index),
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
            member_count_offset: draft.member_count_offset,
            auxiliary_records: draft.auxiliary_records,
            auxiliary_paths: draft.auxiliary_paths,
            trailing_records: draft.trailing_records.into_iter().next(),
            trailing_transforms: draft.trailing_transforms,
            trailing_dual_transforms: draft.trailing_dual_transforms,
            trailing_flags: draft.trailing_flags,
            opaque_index,
            opaque_index_offset: draft.opaque_index_offset,
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
    pub fn role_offset(&self) -> u64 {
        self.opaque_index_offset - 18
    }
    /// First opaque-index location.
    pub fn opaque_index_offset(&self) -> u64 {
        self.opaque_index_offset
    }
    /// Scalar location following the opaque index.
    pub fn opaque_scalar_offset(&self) -> u64 {
        self.opaque_index_offset + 4
    }
    /// Finite nonnegative scalar.
    pub fn opaque_scalar(&self) -> f64 {
        self.opaque_scalar
    }
    /// Zero or one trailing reference.
    pub fn trailing_records(&self) -> &[Located<u32>] {
        self.trailing_records.as_slice()
    }
    /// Distinct auxiliary path records.
    pub fn auxiliary_paths(&self) -> &[DesignConstructionOperandPath] {
        &self.auxiliary_paths
    }
    /// Checked replacement of auxiliary path records.
    pub(crate) fn try_set_auxiliary_paths(
        &mut self,
        records: Vec<DesignConstructionOperandPath>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "auxiliary_paths",
            records.iter().map(|record| record.record_index),
        )?;
        self.auxiliary_paths = records;
        Ok(())
    }
    /// Distinct trailing affine transforms.
    pub fn trailing_transforms(&self) -> &[DesignConstructionOperandTransform] {
        &self.trailing_transforms
    }
    /// Checked replacement of trailing affine transforms.
    pub(crate) fn try_set_trailing_transforms(
        &mut self,
        records: Vec<DesignConstructionOperandTransform>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_transforms",
            records.iter().map(|record| record.record_index),
        )?;
        self.trailing_transforms = records;
        Ok(())
    }
    /// Distinct trailing dual transforms.
    pub fn trailing_dual_transforms(&self) -> &[DesignConstructionOperandDualTransform] {
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
    pub fn trailing_flags(&self) -> &[DesignConstructionOperandFlag] {
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
struct DesignConstructionOperandGroupFrameWire {
    member_count_offset: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_record_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_paths: Vec<DesignConstructionOperandPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_record_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_transforms: Vec<DesignConstructionOperandTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_flags: Vec<DesignConstructionOperandFlag>,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    variant: bool,
}

impl TryFrom<DesignConstructionOperandGroupFrameWire> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandGroupFrameWire) -> Result<Self, Self::Error> {
        if wire.auxiliary_record_indices.len() != wire.auxiliary_record_offsets.len() {
            return Err("auxiliary_record_offsets must match auxiliary_record_indices".into());
        }
        if wire.trailing_record_indices.len() != wire.trailing_record_offsets.len() {
            return Err("trailing_record_offsets must match trailing_record_indices".into());
        }
        Self::try_from(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: wire.member_count_offset,
            auxiliary_records: wire
                .auxiliary_record_indices
                .into_iter()
                .zip(wire.auxiliary_record_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            auxiliary_paths: wire.auxiliary_paths,
            trailing_records: wire
                .trailing_record_indices
                .into_iter()
                .zip(wire.trailing_record_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            trailing_transforms: wire.trailing_transforms,
            trailing_dual_transforms: wire.trailing_dual_transforms,
            trailing_flags: wire.trailing_flags,
            opaque_index: wire.opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: wire.opaque_scalar,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            variant: wire.variant,
        })
    }
}

impl From<DesignConstructionOperandGroupFrame> for DesignConstructionOperandGroupFrameWire {
    fn from(frame: DesignConstructionOperandGroupFrame) -> Self {
        let opaque_scalar_offset = frame.opaque_scalar_offset();
        let opaque_scalar = frame.opaque_scalar();
        let opaque_index_offset = frame.opaque_index_offset();
        Self {
            member_count_offset: frame.member_count_offset,
            auxiliary_record_indices: frame
                .auxiliary_records
                .iter()
                .map(|record| record.value)
                .collect(),
            auxiliary_record_offsets: frame
                .auxiliary_records
                .iter()
                .map(|record| record.offset)
                .collect(),
            auxiliary_paths: frame.auxiliary_paths,
            trailing_record_indices: frame
                .trailing_records
                .iter()
                .map(|record| record.value)
                .collect(),
            trailing_record_offsets: frame
                .trailing_records
                .iter()
                .map(|record| record.offset)
                .collect(),
            trailing_transforms: frame.trailing_transforms,
            trailing_dual_transforms: frame.trailing_dual_transforms,
            trailing_flags: frame.trailing_flags,
            opaque_index: frame.opaque_index.get(),
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset,
            variant: frame.variant,
        }
    }
}

/// Compact boolean record named by a construction-operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignConstructionOperandFlag {
    /// Indexed flag-record identity.
    pub record_index: u32,
    /// Flag-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic flag-record class tag.
    pub class_tag: DesignClassTag,
    /// Stored boolean value.
    pub value: bool,
    /// Byte offset of the stored boolean.
    pub value_offset: u64,
}

/// Affine placement named by a construction-operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignConstructionOperandTransform {
    /// Indexed transform-record identity.
    pub record_index: u32,
    /// Transform-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub class_tag: DesignClassTag,
    /// Row-major local-to-model affine transform.
    pub transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub transform_offset: u64,
    /// Indexed record immediately following the transform.
    pub following_record_index: u32,
    /// Following-record header byte offset.
    pub following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub following_class_tag: DesignClassTag,
}

/// Two ordered affine placements named by an operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignConstructionOperandDualTransform {
    /// Indexed transform-record identity.
    pub record_index: u32,
    /// Transform-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub class_tag: DesignClassTag,
    /// First row-major affine transform.
    pub first_transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub first_transform_offset: u64,
    /// Second row-major affine transform.
    pub second_transform: SketchPlacementMatrix,
    /// Byte offset of the second matrix scalar.
    pub second_transform_offset: u64,
}

/// One persistent-entity step in a construction operand's selection path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignConstructionOperandPathWire",
    into = "DesignConstructionOperandPathWire"
)]
pub struct DesignConstructionOperandPath {
    /// Indexed path-record identity.
    pub record_index: u32,
    /// Path-record header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    pub class_tag: DesignClassTag,
    /// Persistent entity identity carried by this path step.
    pub entity_ref: u64,
    /// Byte offset of `entity_ref`.
    pub entity_ref_offset: u64,
    /// Transform or compact selection-path layout.
    pub placement: DesignConstructionPathPlacement,
    /// Owning feature-scope record.
    pub scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    pub scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    pub nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    pub nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    pub following_record_index: u32,
    /// Following-record header byte offset.
    pub following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub following_class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionOperandPathWire {
    /// Indexed path-record identity.
    record_index: u32,
    /// Path-record header byte offset.
    byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    class_tag: String,
    /// Persistent entity identity carried by this path step.
    entity_ref: u64,
    /// Byte offset of `entity_ref`.
    entity_ref_offset: u64,
    /// Optional row-major selection-path placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the first transform scalar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform_offset: Option<u64>,
    /// Compact-frame boolean; absent from the transform frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compact_variant: Option<bool>,
    /// Owning feature-scope record.
    scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    following_record_index: u32,
    /// Following-record header byte offset.
    following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    following_class_tag: String,
}

impl TryFrom<DesignConstructionOperandPathWire> for DesignConstructionOperandPath {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandPathWire) -> Result<Self, Self::Error> {
        Ok(Self {
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            entity_ref: wire.entity_ref,
            entity_ref_offset: wire.entity_ref_offset,
            placement: match (wire.transform, wire.transform_offset, wire.compact_variant) {
                (Some(value), Some(offset), None) => DesignConstructionPathPlacement::Transform(Located { value, offset }),
                (None, None, Some(variant)) => DesignConstructionPathPlacement::Compact(variant),
                _ => return Err("transform and transform_offset must occur together and exclude compact_variant; compact_variant is required without transform".into()),
            },
            scope_record_index: wire.scope_record_index,
            scope_record_index_offset: wire.scope_record_index_offset,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag.try_into()?,
        })
    }
}

impl From<DesignConstructionOperandPath> for DesignConstructionOperandPathWire {
    fn from(record: DesignConstructionOperandPath) -> Self {
        let (transform, transform_offset, compact_variant) = match record.placement {
            DesignConstructionPathPlacement::Transform(transform) => {
                (Some(transform.value), Some(transform.offset), None)
            }
            DesignConstructionPathPlacement::Compact(variant) => (None, None, Some(variant)),
        };
        Self {
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            entity_ref: record.entity_ref,
            entity_ref_offset: record.entity_ref_offset,
            transform,
            transform_offset,
            compact_variant,
            scope_record_index: record.scope_record_index,
            scope_record_index_offset: record.scope_record_index_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            following_record_index: record.following_record_index,
            following_byte_offset: record.following_byte_offset,
            following_class_tag: record.following_class_tag.into(),
        }
    }
}

/// Placement layout carried by a persistent-entity selection path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DesignConstructionPathPlacement {
    Transform(Located<SketchPlacementMatrix>),
    Compact(bool),
}

/// Nested identity chain named by a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignConstructionOperandIdentityWire",
    into = "DesignConstructionOperandIdentityWire"
)]
pub struct DesignConstructionOperandIdentity {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning operand-group record.
    pub group_record_index: u32,
    /// Ordered identity-wrapper indexed records.
    pub wrappers: Vec<DesignIdentityWrapper>,
    /// Indexed identity of the record physically following the wrappers.
    pub following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    pub following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    pub following_class_tag: DesignClassTag,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

/// Identity and location of one indexed construction wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignIdentityWrapper {
    pub record_index: u32,
    pub byte_offset: u64,
    pub class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionOperandIdentityWire {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning operand-group record.
    pub group_record_index: u32,
    /// Ordered identity-wrapper indexed-record identities.
    pub wrapper_record_indices: Vec<u32>,
    /// Indexed-header byte offsets parallel to `wrapper_record_indices`.
    pub wrapper_byte_offsets: Vec<u64>,
    /// Per-file dynamic class tags parallel to `wrapper_record_indices`.
    pub wrapper_class_tags: Vec<String>,
    /// Indexed identity of the record physically following the wrappers.
    pub following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    pub following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    pub following_class_tag: String,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

impl TryFrom<DesignConstructionOperandIdentityWire> for DesignConstructionOperandIdentity {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandIdentityWire) -> Result<Self, Self::Error> {
        if wire.wrapper_record_indices.len() != wire.wrapper_byte_offsets.len()
            || wire.wrapper_record_indices.len() != wire.wrapper_class_tags.len()
        {
            return Err("wrapper_record_indices, wrapper_byte_offsets, and wrapper_class_tags must have equal lengths".into());
        }
        Ok(Self {
            id: wire.id,
            group_record_index: wire.group_record_index,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag.try_into()?,
            tracking_path: wire.tracking_path,
            persistent_identity: wire.persistent_identity,
            wrappers: wire
                .wrapper_record_indices
                .into_iter()
                .zip(wire.wrapper_byte_offsets)
                .zip(wire.wrapper_class_tags)
                .map(|((record_index, byte_offset), class_tag)| {
                    Ok(DesignIdentityWrapper {
                        record_index,
                        byte_offset,
                        class_tag: class_tag
                            .try_into()
                            .map_err(|error| format!("wrapper_class_tags: {error}"))?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }
}

impl From<DesignConstructionOperandIdentity> for DesignConstructionOperandIdentityWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(identity: DesignConstructionOperandIdentity) -> Self {
        let mut wrapper_record_indices = Vec::with_capacity(identity.wrappers.len());
        let mut wrapper_byte_offsets = Vec::with_capacity(identity.wrappers.len());
        let mut wrapper_class_tags = Vec::with_capacity(identity.wrappers.len());
        for wrapper in identity.wrappers {
            wrapper_record_indices.push(wrapper.record_index);
            wrapper_byte_offsets.push(wrapper.byte_offset);
            wrapper_class_tags.push(wrapper.class_tag.into());
        }
        Self {
            id: identity.id,
            group_record_index: identity.group_record_index,
            following_record_index: identity.following_record_index,
            following_byte_offset: identity.following_byte_offset,
            following_class_tag: identity.following_class_tag.into(),
            tracking_path: identity.tracking_path,
            persistent_identity: identity.persistent_identity,
            wrapper_record_indices,
            wrapper_byte_offsets,
            wrapper_class_tags,
        }
    }
}

/// Entity-tracking path embedded in a construction-operand identity chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignConstructionTrackingPathWire",
    into = "DesignConstructionTrackingPathWire"
)]
pub struct DesignConstructionTrackingPath {
    /// Outer tracking-wrapper record identity.
    pub wrapper_record_index: u32,
    /// Outer tracking-wrapper header byte offset.
    pub wrapper_byte_offset: u64,
    /// Outer tracking-wrapper dynamic class tag.
    pub wrapper_class_tag: DesignClassTag,
    /// Nested tracking-carrier record identity.
    pub carrier_record_index: u32,
    /// Nested tracking-carrier header byte offset.
    pub carrier_byte_offset: u64,
    /// Nested tracking-carrier dynamic class tag.
    pub carrier_class_tag: DesignClassTag,
    /// Primary persistent identity stored by the carrier.
    pub primary_identity: u64,
    /// Byte offset of `primary_identity`.
    pub primary_identity_offset: u64,
    /// Signed carrier selector.
    pub selector: i32,
    /// Byte offset of `selector`.
    pub selector_offset: u64,
    /// Carrier-kind discriminator.
    pub kind: u32,
    /// Byte offset of `kind`.
    pub kind_offset: u64,
    /// First optional related persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_related_identity: Option<Located<u64>>,
    /// Second optional related persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_related_identity: Option<Located<u64>>,
    /// Indexed record immediately following the carrier.
    pub following_record_index: u32,
    /// Following-record header byte offset.
    pub following_byte_offset: u64,
    /// Following-record dynamic class tag.
    pub following_class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionTrackingPathWire {
    wrapper_record_index: u32,
    wrapper_byte_offset: u64,
    wrapper_class_tag: String,
    carrier_record_index: u32,
    carrier_byte_offset: u64,
    carrier_class_tag: String,
    primary_identity: u64,
    primary_identity_offset: u64,
    selector: i32,
    selector_offset: u64,
    kind: u32,
    kind_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_related_identity: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_related_identity_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_related_identity: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_related_identity_offset: Option<u64>,
    following_record_index: u32,
    following_byte_offset: u64,
    following_class_tag: String,
}

impl TryFrom<DesignConstructionTrackingPathWire> for DesignConstructionTrackingPath {
    type Error = String;
    fn try_from(wire: DesignConstructionTrackingPathWire) -> Result<Self, Self::Error> {
        Ok(Self {
            wrapper_record_index: wire.wrapper_record_index,
            wrapper_byte_offset: wire.wrapper_byte_offset,
            wrapper_class_tag: wire.wrapper_class_tag.try_into()?,
            carrier_record_index: wire.carrier_record_index,
            carrier_byte_offset: wire.carrier_byte_offset,
            carrier_class_tag: wire.carrier_class_tag.try_into()?,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            selector: wire.selector,
            selector_offset: wire.selector_offset,
            kind: wire.kind,
            kind_offset: wire.kind_offset,
            first_related_identity: Located::from_wire(
                wire.first_related_identity,
                wire.first_related_identity_offset,
                "first_related_identity",
            )?,
            second_related_identity: Located::from_wire(
                wire.second_related_identity,
                wire.second_related_identity_offset,
                "second_related_identity",
            )?,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag.try_into()?,
        })
    }
}

impl From<DesignConstructionTrackingPath> for DesignConstructionTrackingPathWire {
    fn from(value: DesignConstructionTrackingPath) -> Self {
        Self {
            wrapper_record_index: value.wrapper_record_index,
            wrapper_byte_offset: value.wrapper_byte_offset,
            wrapper_class_tag: value.wrapper_class_tag.into(),
            carrier_record_index: value.carrier_record_index,
            carrier_byte_offset: value.carrier_byte_offset,
            carrier_class_tag: value.carrier_class_tag.into(),
            primary_identity: value.primary_identity,
            primary_identity_offset: value.primary_identity_offset,
            selector: value.selector,
            selector_offset: value.selector_offset,
            kind: value.kind,
            kind_offset: value.kind_offset,
            first_related_identity: value.first_related_identity.map(|located| located.value),
            first_related_identity_offset: value
                .first_related_identity
                .map(|located| located.offset),
            second_related_identity: value.second_related_identity.map(|located| located.value),
            second_related_identity_offset: value
                .second_related_identity
                .map(|located| located.offset),
            following_record_index: value.following_record_index,
            following_byte_offset: value.following_byte_offset,
            following_class_tag: value.following_class_tag.into(),
        }
    }
}

/// Fixed-width persistent identity following a construction-operand identity chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignConstructionPersistentIdentity {
    /// Local persistent identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local identity.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local identity context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub tail_slot_offset: u64,
    /// Identity of the indexed record immediately following this identity.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this identity.
    pub next_byte_offset: u64,
}

/// One radius assignment and its ordered edge group in a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFilletRadiusGroup {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning Fillet scope record.
    pub scope_record_index: u32,
    /// Position among construction-operand groups in scope-reference order.
    pub group_ordinal: u32,
    /// Counted construction-operand group carrying the edges.
    pub group_record_index: u32,
    /// Ordered edge-operand records assigned this radius.
    pub edge_operand_record_indices: Vec<u32>,
    /// Radius law paired with this edge group.
    pub law: DesignFilletRadiusLaw,
    /// Tangency-weight parameter record paired with this edge group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangency_weight_parameter_record_index: Option<u32>,
}

/// Parameter records defining one Fillet group's radius law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFilletRadiusLawWire",
    into = "DesignFilletRadiusLawWire"
)]
pub enum DesignFilletRadiusLaw {
    /// One radius applies along the complete edge group.
    Constant {
        /// Radius parameter record.
        radius_parameter_record_index: u32,
    },
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Chord-length parameter record.
        chord_length_parameter_record_index: u32,
    },
    /// Distinct support-face offsets along the complete edge group.
    Asymmetric {
        /// First support-face offset parameter record.
        offset_one_parameter_record_index: u32,
        /// Second support-face offset parameter record.
        offset_two_parameter_record_index: u32,
    },
    /// Explicit endpoint and optional midpoint radius controls.
    Variable {
        /// Radius at normalized parameter zero.
        start_radius_parameter_record_index: u32,
        /// Radius at normalized parameter one.
        end_radius_parameter_record_index: u32,
        /// Midpoint radius and normalized-parameter records in owner-local order.
        middle: Vec<DesignFilletMidpoint>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignFilletMidpoint {
    pub radius_parameter_record_index: u32,
    pub parameter_record_index: u32,
}

/// Parameter records defining one Fillet group's radius law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignFilletRadiusLawWire {
    /// One radius applies along the complete edge group.
    Constant {
        /// Radius parameter record.
        radius_parameter_record_index: u32,
    },
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Chord-length parameter record.
        chord_length_parameter_record_index: u32,
    },
    /// Distinct support-face offsets along the complete edge group.
    Asymmetric {
        /// First support-face offset parameter record.
        offset_one_parameter_record_index: u32,
        /// Second support-face offset parameter record.
        offset_two_parameter_record_index: u32,
    },
    /// Explicit endpoint and optional midpoint radius controls.
    Variable {
        /// Radius at normalized parameter zero.
        start_radius_parameter_record_index: u32,
        /// Radius at normalized parameter one.
        end_radius_parameter_record_index: u32,
        /// Midpoint radius records in owner-local order.
        middle_radius_parameter_record_indices: Vec<u32>,
        /// Midpoint normalized-parameter records parallel to the radii.
        middle_parameter_record_indices: Vec<u32>,
    },
}

impl TryFrom<DesignFilletRadiusLawWire> for DesignFilletRadiusLaw {
    type Error = String;
    fn try_from(wire: DesignFilletRadiusLawWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignFilletRadiusLawWire::Constant {
                radius_parameter_record_index,
            } => Self::Constant {
                radius_parameter_record_index,
            },
            DesignFilletRadiusLawWire::Chordal {
                chord_length_parameter_record_index,
            } => Self::Chordal {
                chord_length_parameter_record_index,
            },
            DesignFilletRadiusLawWire::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => Self::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            },
            DesignFilletRadiusLawWire::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle_radius_parameter_record_indices,
                middle_parameter_record_indices,
            } => {
                if middle_radius_parameter_record_indices.len()
                    != middle_parameter_record_indices.len()
                {
                    return Err("middle_radius_parameter_record_indices and middle_parameter_record_indices must have equal lengths".into());
                }
                Self::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: middle_radius_parameter_record_indices
                        .into_iter()
                        .zip(middle_parameter_record_indices)
                        .map(|(radius_parameter_record_index, parameter_record_index)| {
                            DesignFilletMidpoint {
                                radius_parameter_record_index,
                                parameter_record_index,
                            }
                        })
                        .collect(),
                }
            }
        })
    }
}

impl From<DesignFilletRadiusLaw> for DesignFilletRadiusLawWire {
    fn from(law: DesignFilletRadiusLaw) -> Self {
        match law {
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index,
            } => Self::Constant {
                radius_parameter_record_index,
            },
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index,
            } => Self::Chordal {
                chord_length_parameter_record_index,
            },
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            } => Self::Asymmetric {
                offset_one_parameter_record_index,
                offset_two_parameter_record_index,
            },
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index,
                end_radius_parameter_record_index,
                middle,
            } => {
                let (middle_radius_parameter_record_indices, middle_parameter_record_indices) =
                    middle
                        .into_iter()
                        .map(|row| {
                            (
                                row.radius_parameter_record_index,
                                row.parameter_record_index,
                            )
                        })
                        .unzip();
                Self::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle_radius_parameter_record_indices,
                    middle_parameter_record_indices,
                }
            }
        }
    }
}

/// ASM history family, entity slot, and states for one selected identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalBinding {
    /// Stable ASM family containing the selected identity.
    #[serde(rename = "historical_entity_kind")]
    pub kind: AsmHistoricalEntityKind,
    /// Stable ASM entity slot after record-revision normalization.
    #[serde(rename = "historical_entity_ref")]
    pub entity_ref: i64,
    /// ASM history states containing the identity, in history arena order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "historical_state_ids")]
    pub state_ids: Vec<i64>,
}

#[derive(Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct OptionalHistoricalBindingWire {
    historical_entity_kind: Option<AsmHistoricalEntityKind>,
    historical_entity_ref: Option<i64>,
    #[serde(default)]
    historical_state_ids: Vec<i64>,
}

fn deserialize_historical_binding<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<HistoricalBinding>, D::Error> {
    let wire = OptionalHistoricalBindingWire::deserialize(deserializer)?;
    match (wire.historical_entity_kind, wire.historical_entity_ref) {
        (None, None) if wire.historical_state_ids.is_empty() => Ok(None),
        (Some(kind), Some(entity_ref)) => Ok(Some(HistoricalBinding { kind, entity_ref, state_ids: wire.historical_state_ids })),
        _ => Err(serde::de::Error::custom("historical_entity_kind and historical_entity_ref are required together for historical_state_ids")),
    }
}

/// One fixed-width member named by an Extrude selection group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignExtrudeSelectionMember {
    /// Globally unique deterministic identifier for this native member.
    pub id: String,
    /// Owning selection-group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Indexed-record identity named by the selection group.
    pub record_index: u32,
    /// Byte offset of the indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Local persistent selection identity preceding the two UUID fields.
    pub local_id: u64,
    /// Byte offset of `local_id`.
    pub local_id_offset: u64,
    /// Asset UUID qualifying the local selection identity.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub tail_slot_offset: u64,
    /// Sketch geometry carrying `local_id`, when it resolves uniquely in
    /// the selected Sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_geometry: Option<SketchRelationOperand>,
    /// Construction-operand identity chains that terminate at this member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operand_identity_ids: Vec<String>,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten, deserialize_with = "deserialize_historical_binding")]
    pub historical: Option<HistoricalBinding>,
    /// Identity of the indexed record immediately following this member.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this member.
    pub next_byte_offset: u64,
}

/// Persistent Design entity selected through a nested indexed-record frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignEntitySelectionOperandWire",
    into = "DesignEntitySelectionOperandWire"
)]
pub struct DesignEntitySelectionOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Owning construction-operand group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
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
    /// Nested indexed record that carries the persistent entity identity.
    pub identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    pub primary_identity: u64,
    /// Byte offset of `primary_identity`.
    pub primary_identity_offset: u64,
    /// Secondary identity and any dependent curve identity, with their source locations.
    pub secondary: Option<DesignSecondaryIdentity<Located<u64>>>,
    /// Input-state edge proofs derived from the two serialized identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_edge_slot: Option<i64>,
    /// Identity of the indexed record immediately following the identity record.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following the identity record.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignEntitySelectionOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning feature scope record.
    scope_record_index: u32,
    /// Owning construction-operand group record.
    group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    group_member_ordinal: u32,
    /// Primary indexed-record identity.
    record_index: u32,
    /// Primary indexed-header byte offset.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the selection namespace.
    asset_id: String,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Nested indexed record that carries the persistent entity identity.
    identity_record_index: u32,
    /// Byte offset of the nested identity record.
    identity_record_offset: u64,
    /// Primary entity identity in the nested identity pair; for a Sketch
    /// curve selection, this is the owning Sketch entity suffix.
    primary_identity: u64,
    /// Byte offset of `primary_identity`.
    primary_identity_offset: u64,
    /// Secondary identity in the nested pair; for a Sketch curve selection,
    /// this is the curve's primary persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity: Option<u64>,
    /// Byte offset of `secondary_identity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_identity_offset: Option<u64>,
    /// Optional secondary identity of the selected Sketch curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity: Option<u64>,
    /// Byte offset of `curve_secondary_identity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_secondary_identity_offset: Option<u64>,
    /// Input-state edge proofs derived from the two serialized identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_edge_candidates: Vec<DesignEntitySelectionEdgeCandidate>,
    /// History-qualified face proofs derived from the primary identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_face_candidates: Vec<DesignEntitySelectionFaceCandidate>,
    /// Unique input-state edge selected by every available identity proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_edge_slot: Option<i64>,
    /// Identity of the indexed record immediately following the identity record.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following the identity record.
    next_byte_offset: u64,
}

impl TryFrom<DesignEntitySelectionOperandWire> for DesignEntitySelectionOperand {
    type Error = String;
    fn try_from(wire: DesignEntitySelectionOperandWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            group_record_index: wire.group_record_index,
            group_member_ordinal: wire.group_member_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id.try_into()?,
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
            historical_edge_candidates: wire.historical_edge_candidates,
            historical_face_candidates: wire.historical_face_candidates,
            resolved_edge_slot: wire.resolved_edge_slot,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignEntitySelectionOperand> for DesignEntitySelectionOperandWire {
    fn from(record: DesignEntitySelectionOperand) -> Self {
        Self {
            id: record.id,
            scope_record_index: record.scope_record_index,
            group_record_index: record.group_record_index,
            group_member_ordinal: record.group_member_ordinal,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            asset_id: record.asset_id.into(),
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id.into(),
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
            historical_edge_candidates: record.historical_edge_candidates,
            historical_face_candidates: record.historical_face_candidates,
            resolved_edge_slot: record.resolved_edge_slot,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Face proof for one persistent identity in one ASM history namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignEntitySelectionFaceCandidate {
    /// Native ASM history containing the selected identity.
    pub history_id: String,
    /// Stable ASM family, entity slot, and states containing the selected identity.
    #[serde(flatten)]
    pub historical: HistoricalBinding,
    /// Face incident to the identity in every listed state.
    pub face_slot: i64,
}

/// Legacy Boolean-Loft body carrier paired with a role-`0x8` body group.
///
/// The carrier is a scope-owned, role-less frame. It is retained separately
/// from the ordinary construction-operand group because its member and
/// scalar lanes do not use the counted-group grammar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignLoftLegacyBodyCarrierSerde",
    into = "DesignLoftLegacyBodyCarrierSerde"
)]
pub struct DesignLoftLegacyBodyCarrier {
    /// Globally unique deterministic identifier.
    pub id: String,
    /// Owning Loft feature scope record.
    pub scope_record_index: u32,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Per-file dynamic primary class tag (`322` or `411`).
    pub class_tag: DesignClassTag,
    /// Byte offset of `owner_scope_record_index`.
    pub owner_scope_record_index_offset: u64,
    /// The one member reference carried by this fixed legacy frame.
    pub member: u32,
    /// Byte offset of `member`.
    pub member_offset: u64,
    /// Byte offset of the on-wire member count (always 1).
    pub member_count_offset: u64,
    /// Opaque nonzero ordinal in the legacy scalar lane.
    pub opaque_index: std::num::NonZeroU8,
    /// Byte offset of the first `opaque_index` copy.
    pub opaque_index_offset: u64,
    /// Opaque finite scalar in the legacy scalar lane.
    pub opaque_scalar: f64,
    /// Byte offset of `opaque_scalar`.
    pub opaque_scalar_offset: u64,
    /// Byte offset of the repeated `opaque_index` copy.
    pub repeated_opaque_index_offset: u64,
    /// Record named by the marked `N+2` reference.
    pub next_next_record_index: u32,
    /// Byte offset of the marked `N+2` reference.
    pub next_next_reference_offset: u64,
    /// Byte offset of `flags`.
    pub flags_offset: u64,
    /// Record named by the marked `N+1` reference.
    pub next_record_index: u32,
    /// Byte offset of the marked `N+1` reference.
    pub next_reference_offset: u64,
    /// Source location of the additional owning-scope reference, when present.
    pub trailing_scope_reference_offset: Option<u64>,
    /// Per-file dynamic paired class tag (`262` or `266`).
    pub paired_class_tag: DesignClassTag,
    /// Same-index paired-header byte offset.
    pub paired_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignLoftLegacyBodyCarrierSerde {
    id: String,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    owner_scope_record_index: u32,
    owner_scope_record_index_offset: u64,
    members: Vec<u32>,
    member_offsets: Vec<u64>,
    member_count: u32,
    member_count_offset: u64,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    repeated_opaque_index: u32,
    repeated_opaque_index_offset: u64,
    next_next_record_index: u32,
    next_next_reference_offset: u64,
    flags: [u8; 2],
    flags_offset: u64,
    next_record_index: u32,
    next_reference_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_scope_record_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trailing_scope_reference_offset: Option<u64>,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignLoftLegacyBodyCarrierSerde> for DesignLoftLegacyBodyCarrier {
    type Error = String;

    fn try_from(wire: DesignLoftLegacyBodyCarrierSerde) -> Result<Self, Self::Error> {
        if wire.scope_reference_ordinal != 0
            || wire.member_count != 1
            || wire.members.len() != 1
            || wire.member_offsets.len() != 1
        {
            return Err(
                "legacy loft body carrier must have one member at scope-reference ordinal zero"
                    .into(),
            );
        }
        if wire.owner_scope_record_index != wire.scope_record_index {
            return Err("owner_scope_record_index disagrees with scope_record_index".into());
        }
        if wire.repeated_opaque_index != wire.opaque_index {
            return Err("repeated_opaque_index disagrees with opaque_index".into());
        }
        if wire.flags != [0, 0] {
            return Err("flags must be zero".into());
        }
        let trailing_scope_reference_offset = match (
            wire.trailing_scope_record_index,
            wire.trailing_scope_reference_offset,
        ) {
            (None, None) => None,
            (Some(index), Some(offset)) if index == wire.scope_record_index => Some(offset),
            (Some(_), Some(_)) => return Err("trailing_scope_record_index disagrees with scope_record_index".into()),
            _ => return Err("trailing_scope_record_index and trailing_scope_reference_offset must occur together".into()),
        };
        let opaque_index = u8::try_from(wire.opaque_index)
            .ok()
            .and_then(std::num::NonZeroU8::new)
            .ok_or("opaque_index must be in 1..=255")?;
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            owner_scope_record_index_offset: wire.owner_scope_record_index_offset,
            member: wire.members[0],
            member_offset: wire.member_offsets[0],
            member_count_offset: wire.member_count_offset,
            opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: wire.opaque_scalar,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            repeated_opaque_index_offset: wire.repeated_opaque_index_offset,
            next_next_record_index: wire.next_next_record_index,
            next_next_reference_offset: wire.next_next_reference_offset,
            flags_offset: wire.flags_offset,
            next_record_index: wire.next_record_index,
            next_reference_offset: wire.next_reference_offset,
            trailing_scope_reference_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

impl From<DesignLoftLegacyBodyCarrier> for DesignLoftLegacyBodyCarrierSerde {
    fn from(carrier: DesignLoftLegacyBodyCarrier) -> Self {
        Self {
            id: carrier.id,
            scope_record_index: carrier.scope_record_index,
            scope_reference_ordinal: 0,
            record_index: carrier.record_index,
            byte_offset: carrier.byte_offset,
            class_tag: carrier.class_tag.into(),
            owner_scope_record_index: carrier.scope_record_index,
            owner_scope_record_index_offset: carrier.owner_scope_record_index_offset,
            members: vec![carrier.member],
            member_offsets: vec![carrier.member_offset],
            member_count: 1,
            member_count_offset: carrier.member_count_offset,
            opaque_index: u32::from(carrier.opaque_index.get()),
            opaque_index_offset: carrier.opaque_index_offset,
            opaque_scalar: carrier.opaque_scalar,
            opaque_scalar_offset: carrier.opaque_scalar_offset,
            repeated_opaque_index: u32::from(carrier.opaque_index.get()),
            repeated_opaque_index_offset: carrier.repeated_opaque_index_offset,
            next_next_record_index: carrier.next_next_record_index,
            next_next_reference_offset: carrier.next_next_reference_offset,
            flags: [0, 0],
            flags_offset: carrier.flags_offset,
            next_record_index: carrier.next_record_index,
            next_reference_offset: carrier.next_reference_offset,
            trailing_scope_record_index: carrier
                .trailing_scope_reference_offset
                .map(|_| carrier.scope_record_index),
            trailing_scope_reference_offset: carrier.trailing_scope_reference_offset,
            paired_class_tag: carrier.paired_class_tag.into(),
            paired_byte_offset: carrier.paired_byte_offset,
        }
    }
}

/// Historical edge proof carried by one nested entity-selection identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignEntitySelectionEdgeCandidate {
    /// Zero for the first identity and one for the second identity.
    pub identity_ordinal: u32,
    /// Serialized persistent identity.
    pub local_id: u64,
    /// Stable ASM history family containing the identity.
    pub historical_entity_kind: AsmHistoricalEntityKind,
    /// Stable ASM entity slot after record-revision normalization.
    pub historical_entity_ref: i64,
    /// Edges incident to the stable entity in the feature-input topology.
    pub edge_slots: Vec<i64>,
}

/// Whole-body construction operand carrying a persistent body-recipe reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignBodyRecipeOperandWire",
    into = "DesignBodyRecipeOperandWire"
)]
pub struct DesignBodyRecipeOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning feature scope record.
    pub scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    pub owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    pub record_index: u32,
    /// Primary indexed-header byte offset.
    pub byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub class_tag: DesignClassTag,
    /// Asset UUID qualifying the persistent selection namespace.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the selection context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_tail: Option<Located<[u8; 4]>>,
    /// Counted persistent Design references carried by this operand.
    pub references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    pub nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    pub nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    pub recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    pub next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    pub next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignBodyRecipeOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning feature scope record.
    scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    record_index: u32,
    /// Primary indexed-header byte offset.
    byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Asset UUID qualifying the persistent selection namespace.
    asset_id: String,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: u64,
    /// UUID of the selection context.
    context_id: String,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector_tail: Option<[u8; 4]>,
    /// Byte offset of the raw selector-tail member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector_tail_offset: Option<u64>,
    /// Counted persistent Design references carried by this operand.
    references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    next_byte_offset: u64,
}

impl TryFrom<DesignBodyRecipeOperandWire> for DesignBodyRecipeOperand {
    type Error = String;
    fn try_from(wire: DesignBodyRecipeOperandWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            owner: wire.owner,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id.try_into()?,
            context_id_offset: wire.context_id_offset,
            selector_tail: Located::from_wire(
                wire.selector_tail,
                wire.selector_tail_offset,
                "selector_tail",
            )?,
            references: wire.references,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            recipe_id: wire.recipe_id,
            resolved_face_slot: wire.resolved_face_slot,
            resolved_body_state_id: wire.resolved_body_state_id,
            resolved_body_slot: wire.resolved_body_slot,
            resolved_body_face_slots: wire.resolved_body_face_slots,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignBodyRecipeOperand> for DesignBodyRecipeOperandWire {
    fn from(record: DesignBodyRecipeOperand) -> Self {
        Self {
            id: record.id,
            scope_record_index: record.scope_record_index,
            owner: record.owner,
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            asset_id: record.asset_id.into(),
            asset_id_offset: record.asset_id_offset,
            context_id: record.context_id.into(),
            context_id_offset: record.context_id_offset,
            selector_tail: record.selector_tail.map(|tail| tail.value),
            selector_tail_offset: record.selector_tail.map(|tail| tail.offset),
            references: record.references,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            recipe_id: record.recipe_id,
            resolved_face_slot: record.resolved_face_slot,
            resolved_body_state_id: record.resolved_body_state_id,
            resolved_body_slot: record.resolved_body_slot,
            resolved_body_face_slots: record.resolved_body_face_slots,
            next_record_index: record.next_record_index,
            next_byte_offset: record.next_byte_offset,
        }
    }
}

/// Construction-operand group record and member position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DesignOperandGroup {
    /// Owning construction-operand group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
}

/// Exact owner of a construction operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DesignOperandOwner {
    /// Operand named by a counted construction-operand group.
    Group {
        /// Owning construction-operand group record.
        group_record_index: u32,
        /// Zero-based position in the group's ordered member run.
        group_member_ordinal: u32,
    },
    /// Standalone operand named directly by the feature scope reference table.
    ScopeReference {
        /// Zero-based position in the scope's ordered reference table.
        scope_reference_ordinal: u32,
    },
}

impl DesignOperandOwner {
    /// Return the construction-group record and member position, when grouped.
    pub const fn group(self) -> Option<(u32, u32)> {
        match self {
            Self::Group {
                group_record_index,
                group_member_ordinal,
            } => Some((group_record_index, group_member_ordinal)),
            Self::ScopeReference { .. } => None,
        }
    }
}

/// One counted persistent reference inside a whole-body recipe operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignBodyRecipeReference {
    /// Persistent Design reference.
    pub design_reference: u64,
    /// Byte offset of `design_reference`.
    pub design_reference_offset: u64,
    /// Reference-local serialized form discriminator.
    pub form: u32,
    /// Byte offset of `form`.
    pub form_offset: u64,
    /// Solved faces carrying this reference, ordered by face identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Candidate faces present in the owning feature's input topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Input-state bodies containing at least one candidate face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_body_slots: Vec<i64>,
}

/// Stable ASM entity family named by a Design persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsmHistoricalEntityKind {
    /// Body topology slot.
    Body,
    /// Region topology slot.
    Region,
    /// Shell topology slot.
    Shell,
    /// Face topology slot.
    Face,
    /// Loop topology slot.
    Loop,
    /// Coedge topology slot.
    Coedge,
    /// Edge topology slot.
    Edge,
    /// Vertex topology slot.
    Vertex,
    /// Point carrier slot.
    Point,
    /// Surface carrier slot.
    Surface,
    /// Curve carrier slot.
    Curve,
    /// Parametric-curve carrier slot.
    Pcurve,
}

/// Prologue framing of a persistent edge-selection identity.
///
/// The three source framings differ only in the length of the zero run before
/// the presence marker, which fixes where every following field sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignEdgeIdentityLayout {
    /// Twelve-zero prologue; the presence marker sits at byte 23.
    Full,
    /// Eleven-zero prologue; the presence marker sits at byte 22.
    Compact,
    /// Ten-zero prologue; the presence marker sits at byte 21.
    Shortest,
}

impl DesignEdgeIdentityLayout {
    /// Byte offset of the presence marker from the indexed-record header.
    pub(crate) fn marker_offset(self) -> u64 {
        match self {
            Self::Full => 23,
            Self::Compact => 22,
            Self::Shortest => 21,
        }
    }

    /// Byte offset of `local_id` from the indexed-record header.
    pub(crate) fn local_id_offset(self) -> u64 {
        self.marker_offset() + 1
    }

    /// The two shortened framings share the on-wire `compact_layout` flag.
    pub(crate) fn is_compact(self) -> bool {
        !matches!(self, Self::Full)
    }

    fn from_local_id_delta(delta: u64) -> Option<Self> {
        [Self::Full, Self::Compact, Self::Shortest]
            .into_iter()
            .find(|layout| layout.local_id_offset() == delta)
    }
}

/// Persistent selection identity owned by a Fillet or Chamfer operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignEdgeIdentityOperandWire",
    into = "DesignEdgeIdentityOperandWire"
)]
pub struct DesignEdgeIdentityOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Owning construction-operand group record.
    pub group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub group_member_ordinal: u32,
    /// Indexed-record identity named by the construction group.
    pub record_index: u32,
    /// Byte offset of the indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag.
    pub class_tag: DesignClassTag,
    /// Prologue framing, which fixes `local_id_offset` relative to
    /// `byte_offset`.
    pub layout: DesignEdgeIdentityLayout,
    /// Local persistent selection identity preceding the two UUID fields.
    pub local_id: u64,
    /// Asset UUID qualifying the local selection identity.
    pub asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub asset_id_offset: u64,
    /// UUID of the local selection-identity context.
    pub context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub context_id_offset: u64,
    /// Stable ASM history family, entity slot, and states carrying `local_id`.
    pub historical: Option<HistoricalBinding>,
    /// Complete radius-qualified deleted source-edge set proved by the owning
    /// feature transition. The transition-scoped set repeats on each operand.
    pub treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Complete deleted source-edge chain proved by the owning feature
    /// transition. The transition-scoped chain repeats on each operand.
    pub transition_edge_candidates: Vec<i64>,
    /// Ordered deleted treatment edges selected by an embedded bounded-face
    /// rule owned by this operand.
    pub resolved_edge_slots: Vec<i64>,
    /// Unique edge slot selected in the owning feature's preceding state.
    pub resolved_edge_slot: Option<i64>,
    /// Native identity or embedded bounded-face operand proving the resolved
    /// edge selection.
    pub resolution_identity_id: Option<String>,
}

impl DesignEdgeIdentityOperand {
    /// Byte offset of `local_id`, fixed by the prologue framing.
    pub fn local_id_offset(&self) -> u64 {
        self.byte_offset
            .saturating_add(self.layout.local_id_offset())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignEdgeIdentityOperandWire {
    id: String,
    scope_record_index: u32,
    group_record_index: u32,
    group_member_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    #[serde(default)]
    compact_layout: bool,
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(flatten, deserialize_with = "deserialize_historical_binding")]
    historical: Option<HistoricalBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transition_edge_candidates: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_edge_slots: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_edge_slot: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution_identity_id: Option<String>,
}

impl TryFrom<DesignEdgeIdentityOperandWire> for DesignEdgeIdentityOperand {
    type Error = String;

    fn try_from(wire: DesignEdgeIdentityOperandWire) -> Result<Self, Self::Error> {
        let layout = wire
            .local_id_offset
            .checked_sub(wire.byte_offset)
            .and_then(DesignEdgeIdentityLayout::from_local_id_delta)
            .ok_or("local_id_offset does not name an edge-identity prologue framing")?;
        if layout.is_compact() != wire.compact_layout {
            return Err(
                "compact_layout disagrees with the framing local_id_offset names".to_owned(),
            );
        }
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            group_record_index: wire.group_record_index,
            group_member_ordinal: wire.group_member_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            layout,
            local_id: wire.local_id,
            asset_id: wire.asset_id.try_into()?,
            asset_id_offset: wire.asset_id_offset,
            context_id: wire.context_id.try_into()?,
            context_id_offset: wire.context_id_offset,
            historical: wire.historical,
            treatment_radius_candidates: wire.treatment_radius_candidates,
            transition_edge_candidates: wire.transition_edge_candidates,
            resolved_edge_slots: wire.resolved_edge_slots,
            resolved_edge_slot: wire.resolved_edge_slot,
            resolution_identity_id: wire.resolution_identity_id,
        })
    }
}

impl From<DesignEdgeIdentityOperand> for DesignEdgeIdentityOperandWire {
    fn from(operand: DesignEdgeIdentityOperand) -> Self {
        let local_id_offset = operand.local_id_offset();
        Self {
            id: operand.id,
            scope_record_index: operand.scope_record_index,
            group_record_index: operand.group_record_index,
            group_member_ordinal: operand.group_member_ordinal,
            record_index: operand.record_index,
            byte_offset: operand.byte_offset,
            class_tag: operand.class_tag.into(),
            compact_layout: operand.layout.is_compact(),
            local_id: operand.local_id,
            local_id_offset,
            asset_id: operand.asset_id.into(),
            asset_id_offset: operand.asset_id_offset,
            context_id: operand.context_id.into(),
            context_id_offset: operand.context_id_offset,
            historical: operand.historical,
            treatment_radius_candidates: operand.treatment_radius_candidates,
            transition_edge_candidates: operand.transition_edge_candidates,
            resolved_edge_slots: operand.resolved_edge_slots,
            resolved_edge_slot: operand.resolved_edge_slot,
            resolution_identity_id: operand.resolution_identity_id,
        }
    }
}

/// Edge-selection operand owned by an edge-selecting parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignEdgeOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Indexed record containing the edge regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Standard two-side structure decoded from the recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_structure: Option<DesignEdgeRecipeStructure>,
    /// Alternate two-clause structure decoded from a `SurfacePatch` edge
    /// recipe program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_patch_recipe_structure: Option<DesignSurfacePatchRecipeStructure>,
    /// Ordered local topology references when every nonzero root and side scalar
    /// is a valid prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_topology_references: Option<Vec<NonZeroU32>>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology produced by the owning
    /// edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the result candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_boundary_edge_slots: Vec<i64>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning edge-treatment feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Candidate and effective prefix-reference faces in the terminal topology
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_candidate_faces: Vec<FaceId>,
    /// Stable edge slots on the preceding candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_boundary_edge_slots: Vec<i64>,
    /// Stable edge slots on terminal candidate-face boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted or updated by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots deleted by the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_boundary_edge_slots: Vec<i64>,
    /// Preceding boundary-edge slots assigned a different record revision by
    /// the owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated_boundary_edge_slots: Vec<i64>,
    /// Deleted predecessor edges associated with inserted treatment-carrier radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub treatment_radius_candidates: Vec<DesignEdgeTreatmentRadiusCandidate>,
    /// Ordered incident-loop topology for every changed boundary edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Ordered incident-loop topology for terminal candidate-face boundaries
    /// used by a suppressed feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_boundary_edge_contexts: Vec<DesignHistoricalEdgeContext>,
    /// Boundary-edge sets of the prefix-reference faces in the terminal
    /// topology, indexed by zero-based prefix-reference ordinal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_reference_edge_slots: Vec<Vec<i64>>,
    /// Ordered historical topology context for each prefix reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipe_reference_contexts: Vec<DesignEdgeRecipeReferenceContext>,
    /// Topology entries grouped by source selector with evaluation-state edge
    /// context matching the selector's incident-loop boundary counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipe_selectors: Vec<DesignEdgeRecipeSelectorContext>,
    /// Historical topology state against which the edge recipe was evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_state_id: Option<i64>,
    /// Stable historical edge slot proven by the selector/reference candidate
    /// intersection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_edge_slot: Option<i64>,
    /// Selected historical carrier axis, when exact.
    #[serde(
        flatten,
        serialize_with = "serialize_edge_resolved_axis",
        deserialize_with = "deserialize_edge_resolved_axis"
    )]
    pub resolved_axis: Option<DesignAxis>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct EdgeResolvedAxisWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_axis_origin: Option<Point3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_axis_direction: Option<Vector3>,
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
fn serialize_edge_resolved_axis<S: serde::Serializer>(
    axis: &Option<DesignAxis>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    EdgeResolvedAxisWire {
        resolved_axis_origin: axis.map(|axis| axis.origin),
        resolved_axis_direction: axis.map(|axis| axis.direction),
    }
    .serialize(serializer)
}

fn deserialize_edge_resolved_axis<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DesignAxis>, D::Error> {
    let wire = EdgeResolvedAxisWire::deserialize(deserializer)?;
    match (wire.resolved_axis_origin, wire.resolved_axis_direction) {
        (None, None) => Ok(None),
        (Some(origin), Some(direction)) => Ok(Some(DesignAxis { origin, direction })),
        _ => Err(serde::de::Error::custom(
            "resolved_axis_origin and resolved_axis_direction must occur together",
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// One radius-qualified historical edge candidate recovered from an inserted
/// treatment face and its carrier-stable adjacent supports.
pub struct DesignEdgeTreatmentRadiusCandidate {
    /// Deleted stable edge slot shared by the preceding support faces.
    pub edge_slot: i64,
    /// Positive characteristic radius of the inserted treatment carrier.
    pub radius: f64,
}

/// Stable surface-support relation from an active face candidate to the
/// topology preceding its owning feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignHistoricalFaceSupportContext {
    /// Stable slot of the active face candidate.
    pub active_face_slot: i64,
    /// Invariant stable surface-carrier slot.
    pub surface_slot: i64,
    /// Preceding face slots owning the surface carrier.
    pub preceding_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the preceding carrier owners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding owners deleted or updated by the feature transition.
    pub changed_preceding_face_slots: Vec<i64>,
}

/// Historical edge-boundary context for one ordered edge-recipe prefix reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignEdgeRecipeReferenceContext {
    /// Zero-based position in the edge recipe's prefix reference sequence.
    pub reference_ordinal: u32,
    /// Referenced faces present in the owning feature's result topology.
    pub result_faces: Vec<FaceId>,
    /// Ordered loop boundaries of each referenced result face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Stable result edge slots shared by the referenced-face boundaries and
    /// the primary candidate-face boundaries.
    pub result_shared_edge_slots: Vec<i64>,
    /// Referenced faces present in the immediately preceding ASM topology.
    pub preceding_faces: Vec<FaceId>,
    /// Ordered loop boundaries of each referenced preceding face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding faces uniquely owning the surface carriers of the referenced
    /// result faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_support_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the uniquely matched preceding support
    /// faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_support_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Stable edge slots shared by the referenced-face boundaries and the
    /// primary candidate-face boundaries.
    pub shared_edge_slots: Vec<i64>,
    /// Shared edge slots deleted or updated by the owning feature transition.
    pub changed_shared_edge_slots: Vec<i64>,
    /// Changed primary-boundary edges belonging to either a directly
    /// persistent referenced face or its unique preceding surface support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_reference_edge_slots: Vec<i64>,
}

/// Ordered loop topology retained for one historical face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignHistoricalFaceBoundaryContext {
    /// Stable ASM face slot.
    pub face_slot: i64,
    /// Face loops in their serialized membership order.
    pub loops: Vec<DesignHistoricalFaceLoopContext>,
}

/// Ordered topology and available geometry of one historical face loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignHistoricalFaceLoopWire",
    into = "DesignHistoricalFaceLoopWire"
)]
pub struct DesignHistoricalFaceLoopContext {
    pub loop_slot: i64,
    pub boundary: DesignHistoricalLoopBoundary,
}

/// Complete runs of the available loop member bindings.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignHistoricalLoopBoundary {
    Coedges(Vec<DesignHistoricalLoopCoedge>),
    Vertices(Vec<DesignHistoricalLoopVertex>),
    Points(Vec<DesignHistoricalLoopPoint>),
    Positions(Vec<DesignHistoricalLoopPosition>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopCoedge {
    pub coedge_slot: i64,
    pub edge_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopVertex {
    pub coedge: DesignHistoricalLoopCoedge,
    pub vertex_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopPoint {
    pub vertex: DesignHistoricalLoopVertex,
    pub point_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopPosition {
    pub point: DesignHistoricalLoopPoint,
    pub position: Point3,
}

impl DesignHistoricalLoopBoundary {
    pub(crate) fn coedges(&self) -> impl Iterator<Item = &DesignHistoricalLoopCoedge> {
        let (mut coedges, mut vertices, mut points, mut positions): (&[_], &[_], &[_], &[_]) =
            (&[], &[], &[], &[]);
        match self {
            Self::Coedges(rows) => coedges = rows,
            Self::Vertices(rows) => vertices = rows,
            Self::Points(rows) => points = rows,
            Self::Positions(rows) => positions = rows,
        }
        coedges
            .iter()
            .chain(vertices.iter().map(|row| &row.coedge))
            .chain(points.iter().map(|row| &row.vertex.coedge))
            .chain(positions.iter().map(|row| &row.point.vertex.coedge))
    }
}

/// Ordered coedge and edge membership of one historical face loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHistoricalFaceLoopWire {
    /// Stable ASM loop slot.
    loop_slot: i64,
    /// Stable coedge slots in cyclic loop order.
    coedge_slots: Vec<i64>,
    /// Stable edge slots aligned one-to-one with `coedge_slots`.
    edge_slots: Vec<i64>,
    /// Stable boundary-vertex slots preceding the aligned coedges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_slots: Vec<i64>,
    /// Stable point-carrier slots aligned one-to-one with `vertex_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    point_slots: Vec<i64>,
    /// Model-space positions aligned one-to-one with `point_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    positions: Vec<cadmpeg_ir::math::Point3>,
}

impl TryFrom<DesignHistoricalFaceLoopWire> for DesignHistoricalFaceLoopContext {
    type Error = String;

    fn try_from(wire: DesignHistoricalFaceLoopWire) -> Result<Self, Self::Error> {
        let count = wire.coedge_slots.len();
        if wire.edge_slots.len() != count {
            return Err("coedge_slots and edge_slots must have equal lengths".into());
        }
        if !wire.vertex_slots.is_empty() && wire.vertex_slots.len() != count {
            return Err("vertex_slots must be empty or match coedge_slots".into());
        }
        if !wire.point_slots.is_empty() && wire.point_slots.len() != wire.vertex_slots.len() {
            return Err("point_slots must be empty or match vertex_slots".into());
        }
        if !wire.positions.is_empty() && wire.positions.len() != wire.point_slots.len() {
            return Err("positions must be empty or match point_slots".into());
        }
        let coedges =
            wire.coedge_slots
                .into_iter()
                .zip(wire.edge_slots)
                .map(|(coedge_slot, edge_slot)| DesignHistoricalLoopCoedge {
                    coedge_slot,
                    edge_slot,
                });
        let boundary = if wire.vertex_slots.is_empty() {
            DesignHistoricalLoopBoundary::Coedges(coedges.collect())
        } else {
            let vertices = coedges.zip(wire.vertex_slots).map(|(coedge, vertex_slot)| {
                DesignHistoricalLoopVertex {
                    coedge,
                    vertex_slot,
                }
            });
            if wire.point_slots.is_empty() {
                DesignHistoricalLoopBoundary::Vertices(vertices.collect())
            } else {
                let points = vertices
                    .zip(wire.point_slots)
                    .map(|(vertex, point_slot)| DesignHistoricalLoopPoint { vertex, point_slot });
                if wire.positions.is_empty() {
                    DesignHistoricalLoopBoundary::Points(points.collect())
                } else {
                    DesignHistoricalLoopBoundary::Positions(
                        points
                            .zip(wire.positions)
                            .map(|(point, position)| DesignHistoricalLoopPosition {
                                point,
                                position,
                            })
                            .collect(),
                    )
                }
            }
        };
        Ok(Self {
            loop_slot: wire.loop_slot,
            boundary,
        })
    }
}

impl From<DesignHistoricalFaceLoopContext> for DesignHistoricalFaceLoopWire {
    fn from(context: DesignHistoricalFaceLoopContext) -> Self {
        let mut wire = Self {
            loop_slot: context.loop_slot,
            coedge_slots: Vec::new(),
            edge_slots: Vec::new(),
            vertex_slots: Vec::new(),
            point_slots: Vec::new(),
            positions: Vec::new(),
        };
        match context.boundary {
            DesignHistoricalLoopBoundary::Coedges(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.coedge_slot);
                    wire.edge_slots.push(row.edge_slot);
                }
            }
            DesignHistoricalLoopBoundary::Vertices(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.coedge.coedge_slot);
                    wire.edge_slots.push(row.coedge.edge_slot);
                    wire.vertex_slots.push(row.vertex_slot);
                }
            }
            DesignHistoricalLoopBoundary::Points(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.vertex.coedge.coedge_slot);
                    wire.edge_slots.push(row.vertex.coedge.edge_slot);
                    wire.vertex_slots.push(row.vertex.vertex_slot);
                    wire.point_slots.push(row.point_slot);
                }
            }
            DesignHistoricalLoopBoundary::Positions(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.point.vertex.coedge.coedge_slot);
                    wire.edge_slots.push(row.point.vertex.coedge.edge_slot);
                    wire.vertex_slots.push(row.point.vertex.vertex_slot);
                    wire.point_slots.push(row.point.point_slot);
                    wire.positions.push(row.position);
                }
            }
        }
        wire
    }
}

/// Historical topology surrounding one candidate edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignHistoricalEdgeContext {
    /// Stable ASM edge slot.
    pub edge_slot: i64,
    /// Incident coedge uses in stable coedge-slot order.
    pub incident_loops: Vec<DesignHistoricalEdgeLoopContext>,
}

/// One historical coedge use of a candidate edge and its ordered loop neighbors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignHistoricalEdgeLoopContext {
    /// Stable ASM coedge slot using the candidate edge.
    pub coedge_slot: i64,
    /// Stable ASM owner-loop slot.
    pub loop_slot: i64,
    /// Stable ASM owner-face slot.
    pub face_slot: i64,
    /// Number of coedges in the owner loop.
    pub boundary_edge_count: u32,
    /// Zero-based position of this coedge in the owner loop's ordered membership.
    pub coedge_ordinal: u32,
    /// Stable edge slot used by the preceding coedge.
    pub previous_edge_slot: i64,
    /// Stable edge slot used by the following coedge.
    pub next_edge_slot: i64,
}

/// Edge-recipe topology entries sharing one selector value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignEdgeRecipeSelectorContextWire",
    into = "DesignEdgeRecipeSelectorContextWire"
)]
pub struct DesignEdgeRecipeSelectorContext {
    pub selector: i32,
    pub clauses: Vec<Option<DesignEdgeRecipeSelectorClause>>,
    pub incidence_matching_edge_slots: Vec<i64>,
    pub boundary_count_matching_edge_slots: Vec<i64>,
}

/// One selector entry and the historical edge slots selected by its triplets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignEdgeRecipeSelectorClause {
    pub entry: DesignTopologyRecipeEntry,
    pub triplet_edge_slots: [Vec<i64>; 2],
}

impl DesignEdgeRecipeSelectorContext {
    pub fn unique_incidence_edge_slot(&self) -> Option<i64> {
        match self.incidence_matching_edge_slots.as_slice() {
            [edge] => Some(*edge),
            _ => None,
        }
    }
}

/// Serialized selector context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignEdgeRecipeSelectorContextWire {
    /// Selector value stored in each grouped entry.
    selector: i32,
    /// Entry from each ordered recipe clause; a selector occurs at most once in
    /// one clause.
    clause_entries: Vec<Option<DesignTopologyRecipeEntry>>,
    /// Changed historical edge slots at the loop position named by each of the
    /// two triplets in each present clause entry.
    clause_triplet_edge_slots: Vec<Option<[Vec<i64>; 2]>>,
    /// Changed historical edges satisfying both triplets of every present
    /// clause entry.
    incidence_matching_edge_slots: Vec<i64>,
    /// The sole incidence-compatible historical edge when the matching set is
    /// a singleton.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unique_incidence_edge_slot: Option<i64>,
    /// Changed historical edges whose incident loop counts satisfy every
    /// present clause entry.
    boundary_count_matching_edge_slots: Vec<i64>,
}

impl TryFrom<DesignEdgeRecipeSelectorContextWire> for DesignEdgeRecipeSelectorContext {
    type Error = String;

    fn try_from(wire: DesignEdgeRecipeSelectorContextWire) -> Result<Self, Self::Error> {
        if wire.clause_entries.len() != wire.clause_triplet_edge_slots.len() {
            return Err("clause_triplet_edge_slots must match clause_entries length".into());
        }
        let clauses = wire
            .clause_entries
            .into_iter()
            .zip(wire.clause_triplet_edge_slots)
            .map(|(entry, slots)| match (entry, slots) {
                (None, None) => Ok(None),
                (Some(entry), Some(triplet_edge_slots)) => {
                    Ok(Some(DesignEdgeRecipeSelectorClause {
                        entry,
                        triplet_edge_slots,
                    }))
                }
                _ => Err(
                    "clause_entries and clause_triplet_edge_slots must be present together"
                        .to_owned(),
                ),
            })
            .collect::<Result<_, _>>()?;
        let context = Self {
            selector: wire.selector,
            clauses,
            incidence_matching_edge_slots: wire.incidence_matching_edge_slots,
            boundary_count_matching_edge_slots: wire.boundary_count_matching_edge_slots,
        };
        if context.unique_incidence_edge_slot() != wire.unique_incidence_edge_slot {
            return Err(
                "unique_incidence_edge_slot must name the singleton incidence_matching_edge_slots"
                    .into(),
            );
        }
        Ok(context)
    }
}

impl From<DesignEdgeRecipeSelectorContext> for DesignEdgeRecipeSelectorContextWire {
    fn from(context: DesignEdgeRecipeSelectorContext) -> Self {
        let unique_incidence_edge_slot = context.unique_incidence_edge_slot();
        let (clause_entries, clause_triplet_edge_slots) = context
            .clauses
            .into_iter()
            .map(|clause| match clause {
                Some(clause) => (Some(clause.entry), Some(clause.triplet_edge_slots)),
                None => (None, None),
            })
            .unzip();
        Self {
            selector: context.selector,
            clause_entries,
            clause_triplet_edge_slots,
            incidence_matching_edge_slots: context.incidence_matching_edge_slots,
            unique_incidence_edge_slot,
            boundary_count_matching_edge_slots: context.boundary_count_matching_edge_slots,
        }
    }
}

/// Standard delimiter structure following an edge recipe's common prologue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignEdgeRecipeStructure {
    /// Number of ordered side clauses.
    pub root: i32,
    /// Ordered side clauses.
    pub sides: Vec<DesignTopologyRecipeSide>,
}

/// The alternate two-clause structure used by a fixed-path `SurfacePatch`
/// edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSurfacePatchRecipeStructureWire",
    into = "DesignSurfacePatchRecipeStructureWire"
)]
pub struct DesignSurfacePatchRecipeStructure {
    /// Ordered clauses in the recipe program.
    pub clauses: [DesignSurfacePatchRecipeClause; 2],
}

#[derive(Serialize, Deserialize)]
struct DesignSurfacePatchRecipeStructureWire {
    root: i32,
    clauses: Vec<DesignSurfacePatchRecipeClause>,
}

impl TryFrom<DesignSurfacePatchRecipeStructureWire> for DesignSurfacePatchRecipeStructure {
    type Error = String;

    fn try_from(wire: DesignSurfacePatchRecipeStructureWire) -> Result<Self, Self::Error> {
        if wire.root != 2 {
            return Err("surface patch recipe root must be 2".into());
        }
        let clauses = wire.clauses.try_into().map_err(|_| {
            "surface patch recipe clauses must contain exactly two clauses".to_owned()
        })?;
        Ok(Self { clauses })
    }
}

impl From<DesignSurfacePatchRecipeStructure> for DesignSurfacePatchRecipeStructureWire {
    fn from(value: DesignSurfacePatchRecipeStructure) -> Self {
        Self {
            root: 2,
            clauses: value.clauses.into(),
        }
    }
}

/// One clause in a `SurfacePatch` edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSurfacePatchRecipeClauseWire",
    into = "DesignSurfacePatchRecipeClauseWire"
)]
pub struct DesignSurfacePatchRecipeClause {
    /// Six delimiter-bounded fields before the counted topology payload.
    pub fields: Vec<Vec<i32>>,
    /// Zero-based face-reference ordinals named by the first two fields.
    pub face_reference_ordinals: [u32; 2],
    /// Zero-based edge-reference ordinals named by the third and fifth fields.
    pub edge_reference_ordinals: [u32; 2],
    /// Ordered topology entries in the payload.
    pub entries: Vec<DesignTopologyRecipeEntry>,
}
#[derive(Serialize, Deserialize)]
struct DesignSurfacePatchRecipeClauseWire {
    /// Six delimiter-bounded fields before the counted topology payload.
    fields: Vec<Vec<i32>>,
    /// Zero-based face-reference ordinals named by the first two fields.
    face_reference_ordinals: [u32; 2],
    /// Zero-based edge-reference ordinals named by the third and fifth fields.
    edge_reference_ordinals: [u32; 2],
    /// Number of eight-word topology entries in the payload.
    payload_entry_count: usize,
    /// Ordered topology entries in the payload.
    entries: Vec<DesignTopologyRecipeEntry>,
}
impl TryFrom<DesignSurfacePatchRecipeClauseWire> for DesignSurfacePatchRecipeClause {
    type Error = &'static str;
    fn try_from(wire: DesignSurfacePatchRecipeClauseWire) -> Result<Self, Self::Error> {
        if wire.payload_entry_count != wire.entries.len() {
            return Err("payload_entry_count disagrees with entries");
        }
        Ok(Self {
            fields: wire.fields,
            face_reference_ordinals: wire.face_reference_ordinals,
            edge_reference_ordinals: wire.edge_reference_ordinals,
            entries: wire.entries,
        })
    }
}
impl From<DesignSurfacePatchRecipeClause> for DesignSurfacePatchRecipeClauseWire {
    fn from(value: DesignSurfacePatchRecipeClause) -> Self {
        Self {
            payload_entry_count: value.entries.len(),
            fields: value.fields,
            face_reference_ordinals: value.face_reference_ordinals,
            edge_reference_ordinals: value.edge_reference_ordinals,
            entries: value.entries,
        }
    }
}

/// One delimiter-bounded side clause in a standard edge recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignTopologyRecipeSideWire",
    into = "DesignTopologyRecipeSideWire"
)]
pub struct DesignTopologyRecipeSide {
    /// Second word of the side header.
    pub header_value: i32,
    /// Ordered scalar fields following the side header.
    pub scalars: Vec<i32>,
    /// Exact field program preceding the topology-entry count.
    pub payload_prefix: Vec<i32>,
    /// Ordered eight-word payload entries.
    pub entries: Vec<DesignTopologyRecipeEntry>,
}
#[derive(Serialize, Deserialize)]
struct DesignTopologyRecipeSideWire {
    /// Encoded number of fields after the header count: scalar fields plus the payload.
    field_count: usize,
    /// Second word of the side header.
    header_value: i32,
    /// Ordered scalar fields following the side header.
    scalars: Vec<i32>,
    /// Exact field program preceding the topology-entry count.
    payload_prefix: Vec<i32>,
    /// Encoded number of eight-word topology entries following the field program.
    payload_entry_count: usize,
    /// Ordered eight-word payload entries.
    entries: Vec<DesignTopologyRecipeEntry>,
}
impl TryFrom<DesignTopologyRecipeSideWire> for DesignTopologyRecipeSide {
    type Error = &'static str;
    fn try_from(wire: DesignTopologyRecipeSideWire) -> Result<Self, Self::Error> {
        if wire.field_count != wire.scalars.len() + 1 {
            return Err("field_count disagrees with scalars");
        }
        if wire.payload_entry_count != wire.entries.len() {
            return Err("payload_entry_count disagrees with entries");
        }
        Ok(Self {
            header_value: wire.header_value,
            scalars: wire.scalars,
            payload_prefix: wire.payload_prefix,
            entries: wire.entries,
        })
    }
}
impl From<DesignTopologyRecipeSide> for DesignTopologyRecipeSideWire {
    fn from(value: DesignTopologyRecipeSide) -> Self {
        Self {
            field_count: value.field_count(),
            payload_entry_count: value.entries.len(),
            header_value: value.header_value,
            scalars: value.scalars,
            payload_prefix: value.payload_prefix,
            entries: value.entries,
        }
    }
}
impl DesignTopologyRecipeSide {
    pub(crate) fn field_count(&self) -> usize {
        self.scalars.len() + 1
    }
}

/// One eight-word topology entry in an edge-recipe side clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignTopologyRecipeEntryWire",
    into = "DesignTopologyRecipeEntryWire"
)]
pub struct DesignTopologyRecipeEntry {
    /// Nonnegative clause-local selector, strictly increasing within one clause.
    pub selector: i32,
    /// Number of boundary edges on the referenced face loop.
    pub boundary_edge_count: NonZeroU32,
    /// Two ordered topology triplets.
    pub topology_triplets: [DesignTopologyRecipeTriplet; 2],
}

#[derive(Serialize, Deserialize)]
struct DesignTopologyRecipeEntryWire {
    /// Nonnegative clause-local selector, strictly increasing within one clause.
    selector: i32,
    /// Number of boundary edges on the referenced face loop.
    boundary_edge_count: NonZeroU32,
    /// Two ordered topology triplets.
    topology_triplets: [DesignTopologyRecipeTriplet; 2],
    /// Zero-based boundary-edge ordinal named by both triplets when equal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    common_incident_edge_ordinal: Option<u32>,
}

impl DesignTopologyRecipeEntry {
    /// Return the incident edge ordinal shared by both triplets.
    pub fn common_incident_edge_ordinal(&self) -> Option<u32> {
        self.topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal)
            .filter(|ordinal| {
                self.topology_triplets[1]
                    .incident
                    .map(|incident| incident.ordinal)
                    == Some(*ordinal)
            })
    }
}

impl TryFrom<DesignTopologyRecipeEntryWire> for DesignTopologyRecipeEntry {
    type Error = String;
    fn try_from(wire: DesignTopologyRecipeEntryWire) -> Result<Self, Self::Error> {
        let value = Self {
            selector: wire.selector,
            boundary_edge_count: wire.boundary_edge_count,
            topology_triplets: wire.topology_triplets,
        };
        if value.common_incident_edge_ordinal() != wire.common_incident_edge_ordinal {
            return Err("common_incident_edge_ordinal disagrees with its source fields".into());
        }
        Ok(value)
    }
}

impl From<DesignTopologyRecipeEntry> for DesignTopologyRecipeEntryWire {
    fn from(value: DesignTopologyRecipeEntry) -> Self {
        let common_incident_edge_ordinal = value.common_incident_edge_ordinal();
        Self {
            selector: value.selector,
            boundary_edge_count: value.boundary_edge_count,
            topology_triplets: value.topology_triplets,
            common_incident_edge_ordinal,
        }
    }
}

/// One three-word invariant in an edge-recipe entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignTopologyRecipeTripletWire",
    into = "DesignTopologyRecipeTripletWire"
)]
pub struct DesignTopologyRecipeTriplet {
    /// Equal positive first and third words, not exceeding the containing
    /// entry's boundary-edge count.
    pub outer: NonZeroU32,
    /// Signed middle word retained from the source triplet.
    pub middle: i32,
    /// Incident edge and its side at the encoded vertex, when derived.
    pub incident: Option<DesignTopologyIncident>,
}

#[derive(Serialize, Deserialize)]
struct DesignTopologyRecipeTripletWire {
    /// Equal positive first and third words, not exceeding the containing
    /// entry's boundary-edge count.
    outer: NonZeroU32,
    /// Signed middle word retained from the source triplet.
    middle: i32,
    /// Zero-based loop vertex ordinal encoded by `outer`.
    vertex_ordinal: u32,
    /// Zero-based boundary-edge ordinal incident to `vertex_ordinal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    incident_edge_ordinal: Option<u32>,
    /// Whether the incident edge precedes or follows the vertex in loop order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    incident_side: Option<DesignTopologyIncidentSide>,
}

impl DesignTopologyRecipeTriplet {
    /// Return the zero-based vertex ordinal encoded by the outer word.
    pub fn vertex_ordinal(&self) -> u32 {
        self.outer.get() - 1
    }
}

impl TryFrom<DesignTopologyRecipeTripletWire> for DesignTopologyRecipeTriplet {
    type Error = String;
    fn try_from(wire: DesignTopologyRecipeTripletWire) -> Result<Self, Self::Error> {
        let incident = match (wire.incident_edge_ordinal, wire.incident_side) {
            (None, None) => None,
            (Some(ordinal), Some(side)) => Some(DesignTopologyIncident { ordinal, side }),
            _ => return Err("incident_edge_ordinal and incident_side must occur together".into()),
        };
        let value = Self {
            outer: wire.outer,
            middle: wire.middle,
            incident,
        };
        if value.vertex_ordinal() != wire.vertex_ordinal {
            return Err("vertex_ordinal disagrees with its source fields".into());
        }
        Ok(value)
    }
}

impl From<DesignTopologyRecipeTriplet> for DesignTopologyRecipeTripletWire {
    fn from(value: DesignTopologyRecipeTriplet) -> Self {
        let vertex_ordinal = value.vertex_ordinal();
        Self {
            outer: value.outer,
            middle: value.middle,
            incident_edge_ordinal: value.incident.map(|incident| incident.ordinal),
            incident_side: value.incident.map(|incident| incident.side),
            vertex_ordinal,
        }
    }
}

/// One incident boundary edge and its side at the selected vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignTopologyIncident {
    pub ordinal: u32,
    pub side: DesignTopologyIncidentSide,
}

/// Which loop edge incident to a recipe vertex is named by a topology triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignTopologyIncidentSide {
    /// Edge immediately preceding the vertex in cyclic loop order.
    Preceding,
    /// Edge immediately following the vertex in cyclic loop order.
    Following,
}

/// Face-selection operand owned by a parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignFaceOperandWire", into = "DesignFaceOperandWire")]
pub struct DesignFaceOperand {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Owning construction-operand group, absent for a direct scope operand.
    pub group: Option<DesignOperandGroup>,
    /// Primary indexed-record identity named by a face operand group.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Indexed record containing the face regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Ordered nodes partitioning the program after its three-word header.
    pub recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces not explicitly named as topology context by a prefix
    /// selector carrying the recipe's own Design reference.
    pub unreferenced_candidate_faces: Vec<FaceId>,
    /// Faces named by a prefix operand carrying the recipe's own token and
    /// Design reference under a different native selector.
    pub alternate_selector_candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    pub changed_candidate_faces: Vec<FaceId>,
    /// Active candidates mapped through an invariant surface carrier to face
    /// owners in the immediately preceding historical topology.
    pub historical_support_contexts: Vec<DesignHistoricalFaceSupportContext>,
    /// Ordered stable historical face slots proven by the preceding topology
    /// or exact feature transition.
    pub resolved_face_slots: Vec<i64>,
    /// Current active-BREP face identity proven by a legacy Extrude recipe
    /// when no preceding historical slot exists.
    pub resolved_active_face: Option<FaceId>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

/// Face-selection operand owned by a parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignFaceOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning parameter-scope record.
    scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    scope_reference_ordinal: u32,
    /// Owning construction-operand group, absent for a direct scope operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_record_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_member_ordinal: Option<u32>,
    /// Primary indexed-record identity named by a face operand group.
    record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Indexed record containing the face regeneration recipe.
    recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    recipe_references: Vec<DesignRecipeReference>,
    /// Exact face-recipe family.
    recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    recipe_program: Vec<i32>,
    /// Byte offsets of the `[-1, -1, 2]` node openers declared by the program.
    recipe_node_offsets: Vec<u64>,
    /// Ordered nodes partitioning the program after its three-word header.
    recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    candidate_faces: Vec<FaceId>,
    /// Candidate faces not explicitly named as topology context by a prefix
    /// selector carrying the recipe's own Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unreferenced_candidate_faces: Vec<FaceId>,
    /// Faces named by a prefix operand carrying the recipe's own token and
    /// Design reference under a different native selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    alternate_selector_candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    changed_candidate_faces: Vec<FaceId>,
    /// Active candidates mapped through an invariant surface carrier to face
    /// owners in the immediately preceding historical topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_support_contexts: Vec<DesignHistoricalFaceSupportContext>,
    /// Ordered stable historical face slots proven by the preceding topology
    /// or exact feature transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_face_slots: Vec<i64>,
    /// Current active-BREP face identity proven by a legacy Extrude recipe
    /// when no preceding historical slot exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved_active_face: Option<FaceId>,
    /// Identity of the indexed record following the operand frame.
    next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    next_byte_offset: u64,
}

impl TryFrom<DesignFaceOperandWire> for DesignFaceOperand {
    type Error = String;
    fn try_from(wire: DesignFaceOperandWire) -> Result<Self, Self::Error> {
        if !wire
            .recipe_node_offsets
            .iter()
            .copied()
            .eq(wire.recipe_nodes.iter().map(|node| node.byte_offset))
        {
            return Err("recipe_node_offsets must match recipe_nodes byte_offset values".into());
        }
        let group = match (wire.group_record_index, wire.group_member_ordinal) {
            (None, None) => None,
            (Some(group_record_index), Some(group_member_ordinal)) => Some(DesignOperandGroup {
                group_record_index,
                group_member_ordinal,
            }),
            _ => {
                return Err(
                    "group_record_index and group_member_ordinal must occur together".into(),
                )
            }
        };
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            group,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            recipe_record_index: wire.recipe_record_index,
            recipe_record_byte_offset: wire.recipe_record_byte_offset,
            recipe_id: wire.recipe_id,
            recipe_prefix_offset: wire.recipe_prefix_offset,
            recipe_prefix_bytes: wire.recipe_prefix_bytes,
            recipe_references: wire.recipe_references,
            recipe_kind: wire.recipe_kind,
            recipe_program_offset: wire.recipe_program_offset,
            recipe_program: wire.recipe_program,
            recipe_nodes: wire.recipe_nodes,
            candidate_faces: wire.candidate_faces,
            unreferenced_candidate_faces: wire.unreferenced_candidate_faces,
            alternate_selector_candidate_faces: wire.alternate_selector_candidate_faces,
            preceding_candidate_faces: wire.preceding_candidate_faces,
            changed_candidate_faces: wire.changed_candidate_faces,
            historical_support_contexts: wire.historical_support_contexts,
            resolved_face_slots: wire.resolved_face_slots,
            resolved_active_face: wire.resolved_active_face,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignFaceOperand> for DesignFaceOperandWire {
    fn from(operand: DesignFaceOperand) -> Self {
        let recipe_node_offsets = operand
            .recipe_nodes
            .iter()
            .map(|node| node.byte_offset)
            .collect();
        Self {
            id: operand.id,
            scope_record_index: operand.scope_record_index,
            scope_reference_ordinal: operand.scope_reference_ordinal,
            group_record_index: operand.group.map(|group| group.group_record_index),
            group_member_ordinal: operand.group.map(|group| group.group_member_ordinal),
            record_index: operand.record_index,
            byte_offset: operand.byte_offset,
            class_tag: operand.class_tag.into(),
            paired_byte_offset: operand.paired_byte_offset,
            paired_class_tag: operand.paired_class_tag.into(),
            recipe_record_index: operand.recipe_record_index,
            recipe_record_byte_offset: operand.recipe_record_byte_offset,
            recipe_id: operand.recipe_id,
            recipe_prefix_offset: operand.recipe_prefix_offset,
            recipe_prefix_bytes: operand.recipe_prefix_bytes,
            recipe_references: operand.recipe_references,
            recipe_kind: operand.recipe_kind,
            recipe_program_offset: operand.recipe_program_offset,
            recipe_program: operand.recipe_program,
            recipe_node_offsets,
            recipe_nodes: operand.recipe_nodes,
            candidate_faces: operand.candidate_faces,
            unreferenced_candidate_faces: operand.unreferenced_candidate_faces,
            alternate_selector_candidate_faces: operand.alternate_selector_candidate_faces,
            preceding_candidate_faces: operand.preceding_candidate_faces,
            changed_candidate_faces: operand.changed_candidate_faces,
            historical_support_contexts: operand.historical_support_contexts,
            resolved_face_slots: operand.resolved_face_slots,
            resolved_active_face: operand.resolved_active_face,
            next_record_index: operand.next_record_index,
            next_byte_offset: operand.next_byte_offset,
        }
    }
}

impl DesignFaceOperand {
    pub(crate) fn group_record_index(&self) -> Option<u32> {
        self.group.map(|group| group.group_record_index)
    }

    pub(crate) fn group_member_ordinal(&self) -> Option<u32> {
        self.group.map(|group| group.group_member_ordinal)
    }
}

/// Native source-shape carrier owned by a `Face` parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFaceSourceGroupWire",
    into = "DesignFaceSourceGroupWire"
)]
pub struct DesignFaceSourceGroup {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning `Face` parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position of the source carrier in the scope reference table.
    pub carrier_reference_ordinal: u32,
    /// Indexed record carrying the ordered source-shape references.
    pub carrier_record_index: u32,
    /// Source interval from the carrier header to its paired header.
    pub carrier_span: NonEmptyByteSpan,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub carrier_class_tag: DesignClassTag,
    /// Indexed record paired with the source carrier.
    pub paired_record_index: u32,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Ordered persistent source-shape identities.
    pub source_members: Vec<Located<DesignFaceSourceMember>>,
}

/// Native source-shape carrier owned by a `Face` parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignFaceSourceGroupWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Owning `Face` parameter-scope record.
    scope_record_index: u32,
    /// Zero-based position of the source carrier in the scope reference table.
    carrier_reference_ordinal: u32,
    /// Indexed record carrying the ordered source-shape references.
    carrier_record_index: u32,
    /// Byte offset of the source carrier's indexed header.
    carrier_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    carrier_class_tag: String,
    /// Bytes from the carrier header to its paired carrier header.
    carrier_frame_length: u64,
    /// Indexed record paired with the source carrier.
    paired_record_index: u32,
    /// Byte offset of the paired carrier's indexed header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Absolute byte offsets of the marked source-reference slots.
    source_reference_offsets: Vec<u64>,
    /// Ordered persistent source-shape identities.
    source_members: Vec<DesignFaceSourceMember>,
}

impl TryFrom<DesignFaceSourceGroupWire> for DesignFaceSourceGroup {
    type Error = String;
    fn try_from(wire: DesignFaceSourceGroupWire) -> Result<Self, Self::Error> {
        if wire.source_members.len() != wire.source_reference_offsets.len() {
            return Err(
                "source_members and source_reference_offsets must have equal lengths".into(),
            );
        }
        let carrier_span = NonEmptyByteSpan::new(wire.carrier_byte_offset, wire.paired_byte_offset)
            .ok_or("paired_byte_offset must follow carrier_byte_offset")?;
        if wire.carrier_frame_length != carrier_span.byte_len() {
            return Err("carrier_frame_length must match the carrier byte span".into());
        }
        Ok(Self {
            carrier_span,
            source_members: wire
                .source_members
                .into_iter()
                .zip(wire.source_reference_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            carrier_reference_ordinal: wire.carrier_reference_ordinal,
            carrier_record_index: wire.carrier_record_index,
            carrier_class_tag: wire.carrier_class_tag.try_into()?,
            paired_record_index: wire.paired_record_index,
            paired_class_tag: wire.paired_class_tag.try_into()?,
        })
    }
}

impl From<DesignFaceSourceGroup> for DesignFaceSourceGroupWire {
    fn from(group: DesignFaceSourceGroup) -> Self {
        let (source_members, source_reference_offsets) = group
            .source_members
            .into_iter()
            .map(|member| (member.value, member.offset))
            .unzip();
        Self {
            source_members,
            source_reference_offsets,
            id: group.id,
            scope_record_index: group.scope_record_index,
            carrier_reference_ordinal: group.carrier_reference_ordinal,
            carrier_record_index: group.carrier_record_index,
            carrier_byte_offset: group.carrier_span.start(),
            carrier_class_tag: group.carrier_class_tag.into(),
            carrier_frame_length: group.carrier_span.byte_len(),
            paired_record_index: group.paired_record_index,
            paired_byte_offset: group.carrier_span.end(),
            paired_class_tag: group.paired_class_tag.into(),
        }
    }
}

/// Persistent source-shape identity named by a `Face` source carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFaceSourceMember {
    /// Indexed record named by the carrier's source-reference slot.
    pub record_index: u32,
    /// Byte offset of the persistent-identity record's indexed header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII identity class tag.
    pub class_tag: DesignClassTag,
    /// Fixed persistent identity carried by the source record.
    pub persistent_identity: DesignConstructionPersistentIdentity,
}

/// One length-delimited node in a face regeneration recipe program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFaceRecipeNode {
    /// Byte offset of the node's `[-1, -1, 2]` opener.
    pub byte_offset: u64,
    /// Exclusive byte offset of the next node or the operand's following record.
    pub end_byte_offset: u64,
    /// Complete node words, including the three-word opener.
    pub program: Vec<i32>,
    /// Shared two-side topology recipe structure following the node opener.
    pub recipe_structure: Option<DesignFaceRecipeStructure>,
}

/// Structured topology program following a face-recipe node opener.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFaceRecipeStructure {
    /// Scalar before the prelude delimiters.
    pub root: i32,
    /// Two scalar prelude runs before the first side clause.
    pub prelude: [i32; 2],
    /// Two ordered topology side clauses.
    pub sides: [DesignTopologyRecipeSide; 2],
    /// Scalar carried by the optional `[-1, value, -1, 0, 0, -1]` postlude.
    #[serde(
        default,
        rename = "postlude",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_face_recipe_postlude",
        deserialize_with = "deserialize_face_recipe_postlude"
    )]
    pub postlude_value: Option<i32>,
}

// The wire adapter receives the optional field by reference, including its absence.
// Serde passes the field by reference to this wire adapter.
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn serialize_face_recipe_postlude<S: serde::Serializer>(
    value: &Option<i32>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => [-1, *value, -1, 0, 0, -1].as_slice().serialize(serializer),
        None => <[i32]>::serialize(&[], serializer),
    }
}

fn deserialize_face_recipe_postlude<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<i32>, D::Error> {
    let words = Vec::<i32>::deserialize(deserializer)?;
    match words.as_slice() {
        [] => Ok(None),
        [-1, value, -1, 0, 0, -1] => Ok(Some(*value)),
        _ => Err(serde::de::Error::custom(
            "postlude must be empty or [-1, value, -1, 0, 0, -1]",
        )),
    }
}

#[cfg(test)]
mod tests;
