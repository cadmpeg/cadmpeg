// SPDX-License-Identifier: Apache-2.0
//! Decal assets and images, the canvas records beside them, and the record header they share.

use cadmpeg_core::decode::u64_from_index;

use super::{identity::Located, references::DesignClassTag};
use serde::{Deserialize, Serialize};
const DESIGN_DECAL_FIT_TO_FACES_CODE: u8 = 0x60;

/// A Decal mapping byte other than the fit-to-faces code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnrecognizedDecalMappingMode(u8);

impl TryFrom<u8> for UnrecognizedDecalMappingMode {
    type Error = String;
    fn try_from(code: u8) -> Result<Self, Self::Error> {
        if code == DESIGN_DECAL_FIT_TO_FACES_CODE {
            Err("mapping_mode unknown payload must exclude the fit-to-faces code".into())
        } else {
            Ok(Self(code))
        }
    }
}

/// Decal image mapping-mode byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub(crate) enum DesignDecalMappingMode {
    FitToFaces,
    Unknown(UnrecognizedDecalMappingMode),
}

impl DesignDecalMappingMode {
    #[must_use]
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            DESIGN_DECAL_FIT_TO_FACES_CODE => Self::FitToFaces,
            code => Self::Unknown(UnrecognizedDecalMappingMode(code)),
        }
    }

    #[must_use]
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::FitToFaces => DESIGN_DECAL_FIT_TO_FACES_CODE,
            Self::Unknown(code) => code.0,
        }
    }
}

impl From<u8> for DesignDecalMappingMode {
    fn from(code: u8) -> Self {
        Self::from_code(code)
    }
}

impl From<DesignDecalMappingMode> for u8 {
    fn from(mode: DesignDecalMappingMode) -> Self {
        mode.code()
    }
}

/// Typed image asset and its consecutive image-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignDecalAsset {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: Option<u64>,
    entity_suffix: u32,
    name: String,
}

impl DesignDecalAsset {
    pub(crate) fn new(
        class_tags: [String; 2],
        record_indices: [u32; 2],
        byte_offset: u64,
        entity_suffix: u32,
        name: String,
    ) -> Result<Self, String> {
        let mut asset = Self::content(class_tags, record_indices, entity_suffix, name)?;
        byte_offset
            .checked_add(Self::primary_frame_length())
            .and_then(|offset| offset.checked_add(asset.name_frame_length()))
            .ok_or("asset_byte_offset and asset_name must leave a complete name record")?;
        asset.byte_offset = Some(byte_offset);
        Ok(asset)
    }

    fn content(
        class_tags: [String; 2],
        record_indices: [u32; 2],
        entity_suffix: u32,
        name: String,
    ) -> Result<Self, String> {
        if record_indices[0].checked_add(1) != Some(record_indices[1]) {
            return Err("name_record_index must immediately follow asset_record_index".into());
        }
        for (field, tag) in ["asset_class_tag", "name_class_tag"]
            .into_iter()
            .zip(&class_tags)
        {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{field} must contain printable ASCII characters"));
            }
        }
        if name.is_empty() {
            return Err("asset_name must be nonempty".into());
        }
        u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        Ok(Self {
            class_tags,
            record_index: record_indices[0],
            byte_offset: None,
            entity_suffix,
            name,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn entity_suffix(&self) -> u32 {
        self.entity_suffix
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) const fn primary_frame_length() -> u64 {
        u64_from_index(crate::layout::design_decal_image_asset_record::LEN)
    }
    pub(crate) fn name_frame_length(&self) -> u64 {
        u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
            + 2 * u64_from_index(self.name.encode_utf16().count())
    }
    fn name_record_index(&self) -> u32 {
        self.record_index + 1
    }
}

/// Typed image and target binding owned by one Design `Decal` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignDecalImageWire")]
pub(crate) struct DesignDecalImage {
    pub(crate) id: String,
    scope: Located<u32, Option<u64>>,
    pub(crate) mapping_mode: DesignDecalMappingMode,
    pub(crate) target_group_record_index: u32,
    pub(crate) asset: DesignDecalAsset,
}

#[cfg(test)]
thread_local! {
    pub(super) static DECAL_IMAGE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
impl Clone for DesignDecalImage {
    fn clone(&self) -> Self {
        DECAL_IMAGE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope: self.scope,
            mapping_mode: self.mapping_mode,
            target_group_record_index: self.target_group_record_index,
            asset: self.asset.clone(),
        }
    }
}
impl DesignDecalImage {
    pub(crate) fn new(
        id: String,
        scope: Located<u32>,
        mapping_mode: DesignDecalMappingMode,
        target_group_record_index: u32,
        asset: DesignDecalAsset,
    ) -> Result<Self, String> {
        scope
            .offset
            .checked_add(u64_from_index(
                crate::layout::design_decal_scope_prefix::LEN,
            ))
            .ok_or("asset_reference_offset must belong to a complete Decal scope prefix")?;
        Ok(Self {
            id,
            scope: Located {
                value: scope.value,
                offset: Some(scope.offset),
            },
            mapping_mode,
            target_group_record_index,
            asset,
        })
    }
    pub(crate) fn scope_record_index(&self) -> u32 {
        self.scope.value
    }
}

/// Content only: source offsets and encoded frame extents live in fidelity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignDecalImageWire {
    id: String,
    scope_record_index: u32,
    mapping_mode: DesignDecalMappingMode,
    target_group_record_index: u32,
    asset_class_tag: String,
    asset_record_index: u32,
    asset_entity_suffix: u32,
    name_class_tag: String,
    name_record_index: u32,
    asset_name: String,
}
#[derive(Serialize)]
struct DesignDecalImageWireRef<'a> {
    id: &'a str,
    scope_record_index: u32,
    mapping_mode: DesignDecalMappingMode,
    target_group_record_index: u32,
    asset_class_tag: &'a str,
    asset_record_index: u32,
    asset_entity_suffix: u32,
    name_class_tag: &'a str,
    name_record_index: u32,
    asset_name: &'a str,
}
impl Serialize for DesignDecalImage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignDecalImageWireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index(),
            mapping_mode: self.mapping_mode,
            target_group_record_index: self.target_group_record_index,
            asset_class_tag: &self.asset.class_tags[0],
            asset_record_index: self.asset.record_index,
            asset_entity_suffix: self.asset.entity_suffix,
            name_class_tag: &self.asset.class_tags[1],
            name_record_index: self.asset.name_record_index(),
            asset_name: &self.asset.name,
        }
        .serialize(serializer)
    }
}
impl TryFrom<DesignDecalImageWire> for DesignDecalImage {
    type Error = String;
    fn try_from(wire: DesignDecalImageWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            scope: Located {
                value: wire.scope_record_index,
                offset: None,
            },
            mapping_mode: wire.mapping_mode,
            target_group_record_index: wire.target_group_record_index,
            asset: DesignDecalAsset::content(
                [wire.asset_class_tag, wire.name_class_tag],
                [wire.asset_record_index, wire.name_record_index],
                wire.asset_entity_suffix,
                wire.asset_name,
            )?,
        })
    }
}
#[cfg(test)]
impl From<DesignDecalImage> for DesignDecalImageWire {
    fn from(value: DesignDecalImage) -> Self {
        let name_record_index = value.asset.name_record_index();
        let [asset_class_tag, name_class_tag] = value.asset.class_tags;
        Self {
            id: value.id,
            scope_record_index: value.scope.value,
            mapping_mode: value.mapping_mode,
            target_group_record_index: value.target_group_record_index,
            asset_class_tag,
            asset_record_index: value.asset.record_index,
            asset_entity_suffix: value.asset.entity_suffix,
            name_class_tag,
            name_record_index,
            asset_name: value.asset.name,
        }
    }
}

/// One indexed record header in the recursive Design `BulkStream` tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignRecordHeader {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this record within the recursive `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this header within its Design `BulkStream`.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
}

#[cfg(test)]
mod tests;
