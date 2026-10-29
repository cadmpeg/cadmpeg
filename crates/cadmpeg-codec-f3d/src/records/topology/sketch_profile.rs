// SPDX-License-Identifier: Apache-2.0
//! Sketch-profile selection operands and their profile regions.

use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::records::identity::DesignEntityId;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::u64_from_index;
use serde::Deserialize;
use serde::Serialize;

/// Sketch-profile selection frame named by a profile-based feature scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignSketchProfileOperandWire")]
pub(crate) struct DesignSketchProfileOperand {
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: Option<u64>,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selected Sketch reference.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    asset_id_offset: Option<u64>,
    /// Full Design entity id of the selected Sketch.
    pub(crate) entity_id: DesignEntityId,
    /// Byte offset of the suffix's UTF-16LE code units.
    entity_reference_offset: Option<u64>,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: Option<u64>,
}

#[cfg(test)]
thread_local! {
    static SKETCH_PROFILE_OPERAND_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignSketchProfileOperand {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SKETCH_PROFILE_OPERAND_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            asset_id: self.asset_id.clone(),
            asset_id_offset: self.asset_id_offset,
            entity_id: self.entity_id.clone(),
            entity_reference_offset: self.entity_reference_offset,
            region_selection: self.region_selection.clone(),
            paired_class_tag: self.paired_class_tag.clone(),
            paired_byte_offset: self.paired_byte_offset,
        }
    }
}

impl DesignSketchProfileOperand {
    pub(crate) fn try_new(draft: DesignSketchProfileOperandDraft) -> Result<Self, String> {
        if !(draft.byte_offset < draft.asset_id_offset
            && draft.asset_id_offset < draft.entity_reference_offset
            && draft.entity_reference_offset < draft.paired_byte_offset)
        {
            return Err(
                "asset_id_offset/entity_reference_offset/paired_byte_offset must increase".into(),
            );
        }
        let value = Self {
            scope_reference_ordinal: draft.scope_reference_ordinal,
            record_index: draft.record_index,
            byte_offset: Some(draft.byte_offset),
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            asset_id_offset: Some(draft.asset_id_offset),
            entity_id: draft.entity_id,
            entity_reference_offset: Some(draft.entity_reference_offset),
            region_selection: draft.region_selection,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: Some(draft.paired_byte_offset),
        };
        value.validate_region_semantics()?;
        Ok(value)
    }
    fn validate_region_semantics(&self) -> Result<(), String> {
        if self
            .region_selection
            .as_ref()
            .is_some_and(|selection| !self.region_selection_is_admitted(selection))
        {
            return Err("profile region selection has an invalid logical record, member run, or actual source frame".into());
        }
        Ok(())
    }
    pub(crate) fn region_selection_is_admitted(
        &self,
        selection: &DesignSketchProfileRegionSelection,
    ) -> bool {
        if self.record_index.checked_add(3) != Some(selection.record_index)
            || selection.regions.is_empty()
            || selection
                .regions
                .iter()
                .any(|region| region.members.is_empty())
        {
            return false;
        }
        let Some(byte_offset) = selection.byte_offset else {
            // A typed document carries no physical source positions. Its ordered
            // references and incidence values remain admitted and checked above.
            return selection.region_count_offset.is_none()
                && selection.companion_byte_offset.is_none()
                && selection.regions.iter().all(|region| {
                    region.member_count_offset.is_none()
                        && region.members.iter().all(|member| {
                            member.kind_offset.is_none()
                                && member.curve_primary_id_offset.is_none()
                                && member.incidence_words_offset.is_none()
                        })
                });
        };
        if self
            .paired_byte_offset()
            .is_none_or(|paired| byte_offset <= paired)
            || byte_offset.checked_add(u64_from_index(region_selection::REGION_COUNT))
                != selection.region_count_offset
        {
            return false;
        }
        let Some(mut cursor) = byte_offset.checked_add(u64_from_index(region_selection::LEN))
        else {
            return false;
        };
        for (region_ordinal, region) in selection.regions.iter().enumerate() {
            if region_ordinal != 0 {
                let Some(next) = cursor.checked_add(1) else {
                    return false;
                };
                cursor = next;
            }
            if region.member_count_offset != Some(cursor) {
                return false;
            }
            let Some(next) = cursor.checked_add(4) else {
                return false;
            };
            cursor = next;
            for member in &region.members {
                let (Some(curve_primary_id_offset), Some(incidence_words_offset), Some(next)) = (
                    cursor.checked_add(4),
                    cursor.checked_add(8),
                    cursor.checked_add(40),
                ) else {
                    return false;
                };
                if member.kind_offset != Some(cursor)
                    || member.curve_primary_id_offset != Some(curve_primary_id_offset)
                    || member.incidence_words_offset != Some(incidence_words_offset)
                {
                    return false;
                }
                cursor = next;
            }
        }
        cursor.checked_add(5) == selection.companion_byte_offset
    }

    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignSketchProfileOperandDraft, String> {
        Ok(DesignSketchProfileOperandDraft {
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self
                .byte_offset
                .ok_or("profile operand has no source byte_offset")?,
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset: self
                .asset_id_offset
                .ok_or("profile operand has no source asset_id_offset")?,
            entity_id: self.entity_id,
            entity_reference_offset: self
                .entity_reference_offset
                .ok_or("profile operand has no source entity_reference_offset")?,
            region_selection: self.region_selection,
            paired_class_tag: self.paired_class_tag,
            paired_byte_offset: self
                .paired_byte_offset
                .ok_or("profile operand has no source paired_byte_offset")?,
        })
    }
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }
    pub(crate) fn paired_byte_offset(&self) -> Option<u64> {
        self.paired_byte_offset
    }
}

impl Serialize for DesignSketchProfileOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            scope_reference_ordinal: u32,
            record_index: u32,
            class_tag: &'a str,
            asset_id: &'a str,
            entity_id: &'a str,
            entity_suffix: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            region_selection: Option<&'a DesignSketchProfileRegionSelection>,
            paired_class_tag: &'a str,
        }
        WireRef {
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            class_tag: self.class_tag.as_str(),
            asset_id: self.asset_id.as_str(),
            entity_id: self.entity_id.as_str(),
            entity_suffix: self.entity_id.suffix(),
            region_selection: self.region_selection.as_ref(),
            paired_class_tag: self.paired_class_tag.as_str(),
        }
        .serialize(serializer)
    }
}

/// Unadmitted `DesignSketchProfileOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignSketchProfileOperandDraft {
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selected Sketch reference.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// Full Design entity id of the selected Sketch.
    pub(crate) entity_id: DesignEntityId,
    /// Byte offset of the suffix's UTF-16LE code units.
    pub(crate) entity_reference_offset: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    pub(crate) region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub(crate) paired_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSketchProfileOperandWire {
    /// Zero-based position in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity named by the scope table.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: String,
    /// Asset UUID qualifying the selected Sketch reference.
    pub(crate) asset_id: String,
    /// Full Design entity id of the selected Sketch.
    pub(crate) entity_id: String,
    /// Numeric suffix stored by the profile frame.
    pub(crate) entity_suffix: u64,
    /// Exact nested profile-region selection, when its complete frame closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_region_selection"
    )]
    pub(crate) region_selection: Option<DesignSketchProfileRegionSelection>,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub(crate) paired_class_tag: String,
}

impl TryFrom<DesignSketchProfileOperandWire> for DesignSketchProfileOperand {
    type Error = String;

    fn try_from(wire: DesignSketchProfileOperandWire) -> Result<Self, Self::Error> {
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        if entity_id.suffix() != wire.entity_suffix {
            return Err("entity_suffix disagrees with entity_id".into());
        }
        let value = Self {
            scope_reference_ordinal: wire.scope_reference_ordinal,
            byte_offset: None,
            asset_id_offset: None,
            entity_reference_offset: None,
            paired_byte_offset: None,
            record_index: wire.record_index,
            class_tag: wire.class_tag.try_into()?,
            asset_id: wire.asset_id.try_into()?,
            entity_id,
            region_selection: wire.region_selection,
            paired_class_tag: wire.paired_class_tag.try_into()?,
        };
        value.validate_region_semantics()?;
        Ok(value)
    }
}

#[cfg(test)]
impl From<DesignSketchProfileOperand> for DesignSketchProfileOperandWire {
    fn from(value: DesignSketchProfileOperand) -> Self {
        let entity_suffix = value.entity_id.suffix();
        Self {
            scope_reference_ordinal: value.scope_reference_ordinal,
            record_index: value.record_index,
            class_tag: value.class_tag.into(),
            asset_id: value.asset_id.into(),
            entity_id: value.entity_id.into_string(),
            entity_suffix,
            region_selection: value.region_selection,
            paired_class_tag: value.paired_class_tag.into(),
        }
    }
}

/// Nested ordered region selection carried by a sketch-profile operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSketchProfileRegionSelection {
    /// Indexed identity of the region-selection record.
    pub(crate) record_index: u32,
    /// Byte offset of the region-selection indexed header.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
    /// Source per-file dynamic three-digit ASCII region-selection class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of the selected-region count.
    #[serde(skip)]
    pub(crate) region_count_offset: Option<u64>,
    /// Selected regions in source order.
    pub(crate) regions: Vec<DesignSketchProfileRegion>,
    /// Source per-file dynamic three-digit ASCII companion class tag.
    pub(crate) companion_class_tag: DesignClassTag,
    /// Byte offset of the same-index companion header.
    #[serde(skip)]
    pub(crate) companion_byte_offset: Option<u64>,
}

/// One selected region in a nested sketch-profile selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSketchProfileRegion {
    /// Byte offset of this region's member count.
    #[serde(skip)]
    pub(crate) member_count_offset: Option<u64>,
    /// Persistent curve members in source order.
    pub(crate) members: Vec<DesignSketchProfileRegionMember>,
}

/// One fixed-width persistent curve member of a selected sketch region.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchProfileRegionMemberWire",
    into = "DesignSketchProfileRegionMemberWire"
)]
pub(crate) struct DesignSketchProfileRegionMember {
    /// Byte offset of the member-kind code.
    #[serde(skip)]
    pub(crate) kind_offset: Option<u64>,
    /// Primary persistent identity of the selected Sketch curve.
    pub(crate) curve_primary_id: std::num::NonZeroU32,
    /// Byte offset of the persistent curve identity.
    #[serde(skip)]
    pub(crate) curve_primary_id_offset: Option<u64>,
    /// First incidence value, encoded as zero or one.
    pub(crate) incidence_flag: bool,
    /// Second and third incidence values, in source order.
    pub(crate) incidence_values: [DesignRegionIncidence; 2],
    /// Byte offset of the first incidence word.
    #[serde(skip)]
    pub(crate) incidence_words_offset: Option<u64>,
}

/// One of the two admitted nonzero region-incidence values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignRegionIncidence {
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
#[serde(deny_unknown_fields)]
struct DesignSketchProfileRegionMemberWire {
    /// Native member-kind code. Profile-region curve members use value three.
    kind: u32,
    /// Primary persistent identity of the selected Sketch curve.
    curve_primary_id: u64,
    /// Structural region-incidence words retained in source order.
    incidence_words: [u32; 8],
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
            kind_offset: None,
            curve_primary_id,
            curve_primary_id_offset: None,
            incidence_flag,
            incidence_values: [
                DesignRegionIncidence::try_from(first)?,
                DesignRegionIncidence::try_from(second)?,
            ],
            incidence_words_offset: None,
        })
    }
}

impl From<DesignSketchProfileRegionMember> for DesignSketchProfileRegionMemberWire {
    fn from(member: DesignSketchProfileRegionMember) -> Self {
        Self {
            kind: 3,
            curve_primary_id: u64::from(member.curve_primary_id.get()),
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
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_region_selection,
    DesignSketchProfileRegionSelection,
    "region_selection"
);

#[cfg(test)]
mod tests;

mod identity_rewrite;
