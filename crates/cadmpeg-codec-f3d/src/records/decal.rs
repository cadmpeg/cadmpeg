// SPDX-License-Identifier: Apache-2.0
//! Decal assets and images, the canvas records beside them, and the record header they share.

use cadmpeg_core::decode::u64_from_index;

use super::{identity::Located, references::DesignClassTag};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_placement, DesignDecalPlacement, "placement");

/// Largest deviation of the squared placement normal length from one.
pub(crate) const PLACEMENT_UNIT_NORMAL_TOLERANCE: f64 = 1.0e-9;

/// Largest deviation from one of the absolute cosine between the placement normal and `U x V`.
pub(crate) const PLACEMENT_NORMAL_PARALLEL_TOLERANCE: f64 = 1.0e-6;

/// Placement frame stored by a Decal placement record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignDecalPlacement {
    /// Byte offset of the placement record.
    pub(crate) byte_offset: u64,
    /// Dynamic class tag of the placement record.
    pub(crate) class_tag: String,
    /// Count of the (u, v) pairs, one for each display-mesh vertex of the target body.
    pub(crate) vertex_count: u32,
    /// Decal ordinal.
    pub(crate) ordinal: u32,
    /// Image width axis in Design length units.
    pub(crate) u_axis: [f64; 3],
    /// Image height axis in Design length units.
    pub(crate) v_axis: [f64; 3],
    /// The three bytes that follow `V`.
    pub(crate) flag_bytes: cadmpeg_ir::native::bytes::NativeBytes<[u8; 3]>,
    /// Count of the zero bytes between the two trailing references.
    pub(crate) reference_separator: u8,
    /// Count of the zero bytes between the second trailing reference and the normal.
    pub(crate) normal_gap: u8,
    /// Unit normal of the image plane.
    pub(crate) normal: [f64; 3],
    /// Image centre in Design length units.
    pub(crate) origin: [f64; 3],
    /// Name of the source image.
    pub(crate) image_name: String,
    /// Count of the covered-triangle vertex indices.
    pub(crate) triangle_index_count: u32,
}

impl DesignDecalPlacement {
    /// Whether the normal is parallel to `U x V`, in either direction.
    pub(crate) fn normal_is_parallel_to_axes(&self) -> bool {
        let (u, v, n) = (self.u_axis, self.v_axis, self.normal);
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = cross.iter().map(|value| value * value).sum::<f64>().sqrt();
        let projection: f64 = cross.iter().zip(n).map(|(a, b)| a * b).sum();
        length > 0.0
            && (projection.abs() / length - 1.0).abs() <= PLACEMENT_NORMAL_PARALLEL_TOLERANCE
    }

    /// Whether one zero byte separates the trailing references and four precede the normal.
    pub(crate) fn has_stated_layout(&self) -> bool {
        (self.reference_separator, self.normal_gap) == (1, 4)
    }

    /// Admit a frame that precedes the image-asset record at `asset_byte_offset`.
    pub(crate) fn admit(&self, asset_byte_offset: u64) -> Result<(), String> {
        if self.byte_offset >= asset_byte_offset {
            return Err("placement byte_offset must precede the image-asset record".into());
        }
        if self.class_tag.is_empty() || !self.class_tag.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err("placement class_tag must contain printable ASCII characters".into());
        }
        if self.image_name.is_empty() {
            return Err("placement image_name must be nonempty".into());
        }
        if self.reference_separator > 1 {
            return Err("placement reference_separator must be 0 or 1".into());
        }
        if !matches!(self.normal_gap, 4 | 5) {
            return Err("placement normal_gap must be 4 or 5".into());
        }
        if [self.u_axis, self.v_axis, self.normal, self.origin]
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err("placement vectors must be finite".into());
        }
        let squared_length: f64 = self.normal.iter().map(|value| value * value).sum();
        if (squared_length - 1.0).abs() > PLACEMENT_UNIT_NORMAL_TOLERANCE {
            return Err("placement normal must have unit length".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignDecalScopeLayout {
    Compact,
    Extended,
    TypedCompact,
    TypedExtended,
}

impl DesignDecalScopeLayout {
    pub(crate) const ALL: [Self; 4] = [
        Self::Compact,
        Self::Extended,
        Self::TypedCompact,
        Self::TypedExtended,
    ];

    pub(crate) const fn typed(self) -> bool {
        matches!(self, Self::TypedCompact | Self::TypedExtended)
    }

    pub(crate) const fn extended(self) -> bool {
        matches!(self, Self::Extended | Self::TypedExtended)
    }

    pub(crate) const fn asset_reference(self) -> usize {
        match self {
            Self::Compact => crate::layout::design_decal_scope_prefix::ASSET_REFERENCE,
            Self::Extended => crate::layout::design_decal_scope_extended_prefix::ASSET_REFERENCE,
            Self::TypedCompact => crate::layout::design_decal_scope_typed_prefix::ASSET_REFERENCE,
            Self::TypedExtended => {
                crate::layout::design_decal_scope_typed_extended_prefix::ASSET_REFERENCE
            }
        }
    }

    pub(crate) const fn scope_code(self) -> usize {
        match self {
            Self::Compact => crate::layout::design_decal_scope_prefix::SCOPE_CODE,
            Self::Extended => crate::layout::design_decal_scope_extended_prefix::SCOPE_CODE,
            Self::TypedCompact => crate::layout::design_decal_scope_typed_prefix::SCOPE_CODE,
            Self::TypedExtended => {
                crate::layout::design_decal_scope_typed_extended_prefix::SCOPE_CODE
            }
        }
    }

    pub(crate) const fn target_group_reference(self) -> usize {
        match self {
            Self::Compact => crate::layout::design_decal_scope_prefix::TARGET_GROUP_REFERENCE,
            Self::Extended => {
                crate::layout::design_decal_scope_extended_prefix::TARGET_GROUP_REFERENCE
            }
            Self::TypedCompact => {
                crate::layout::design_decal_scope_typed_prefix::TARGET_GROUP_REFERENCE
            }
            Self::TypedExtended => {
                crate::layout::design_decal_scope_typed_extended_prefix::TARGET_GROUP_REFERENCE
            }
        }
    }

    pub(crate) const fn len(self) -> usize {
        match self {
            Self::Compact => crate::layout::design_decal_scope_prefix::LEN,
            Self::Extended => crate::layout::design_decal_scope_extended_prefix::LEN,
            Self::TypedCompact => crate::layout::design_decal_scope_typed_prefix::LEN,
            Self::TypedExtended => crate::layout::design_decal_scope_typed_extended_prefix::LEN,
        }
    }

    pub(crate) const fn asset_frame_length(self) -> usize {
        if self.typed() {
            crate::layout::design_decal_image_typed_asset_record::LEN
        } else {
            crate::layout::design_decal_image_asset_record::LEN
        }
    }
}

/// Primary Decal asset and its consecutive image-name record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignDecalAsset {
    primary_frame_length: u64,
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: u64,
    entity_suffix: u32,
    name: String,
}

impl DesignDecalAsset {
    pub(crate) fn new(
        layout: DesignDecalScopeLayout,
        class_tags: [String; 2],
        record_indices: [u32; 2],
        byte_offset: u64,
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
        let units = u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        let primary_frame_length = u64_from_index(layout.asset_frame_length());
        let length = primary_frame_length
            + u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
            + 2 * u64::from(units);
        byte_offset
            .checked_add(length)
            .ok_or("asset_byte_offset and asset_name must leave a complete name record")?;
        Ok(Self {
            primary_frame_length,
            class_tags,
            record_index: record_indices[0],
            byte_offset,
            entity_suffix,
            name,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn entity_suffix(&self) -> u32 {
        self.entity_suffix
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    #[cfg(test)]
    pub(crate) fn primary_frame_length(&self) -> u64 {
        self.primary_frame_length
    }
    pub(crate) fn name_frame_length(&self) -> u64 {
        u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
            + 2 * u64_from_index(self.name.encode_utf16().count())
    }
    fn name_record_index(&self) -> u32 {
        self.record_index + 1
    }
    fn name_byte_offset(&self) -> u64 {
        self.byte_offset + self.primary_frame_length
    }
    fn entity_reference_offset(&self) -> u64 {
        self.byte_offset
            + u64_from_index(
                crate::layout::design_decal_image_asset_record::DESIGN_ENTITY_SUFFIX_REFERENCE,
            )
            + 1
    }
    fn name_offset(&self) -> u64 {
        self.name_byte_offset() + u64_from_index(crate::layout::design_decal_image_name_prefix::LEN)
    }
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignDecalImageWire")]
pub(crate) struct DesignDecalImage {
    /// Globally unique native binding identity.
    pub(crate) id: String,
    scope: Located<u32>,
    layout: DesignDecalScopeLayout,
    /// Scope code byte between the image-asset and target-group references.
    pub(crate) scope_code: u8,
    /// Frame of the Decal placement record.
    pub(crate) placement: Option<DesignDecalPlacement>,
    /// Target construction-group record index.
    pub(crate) target_group_record_index: u32,
    /// Consecutive asset and image-name records.
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
            layout: self.layout,
            scope_code: self.scope_code,
            placement: self.placement.clone(),
            target_group_record_index: self.target_group_record_index,
            asset: self.asset.clone(),
        }
    }
}

impl DesignDecalImage {
    pub(crate) fn new(
        id: String,
        scope: Located<u32>,
        layout: DesignDecalScopeLayout,
        scope_code: u8,
        target_group_record_index: u32,
        asset: DesignDecalAsset,
    ) -> Result<Self, String> {
        scope
            .offset
            .checked_add(u64_from_index(layout.len()))
            .ok_or("asset_reference_offset must belong to a complete Decal scope prefix")?;
        if asset.primary_frame_length != u64_from_index(layout.asset_frame_length()) {
            return Err("asset_frame_length must match the Decal scope layout".into());
        }
        Ok(Self {
            id,
            scope,
            layout,
            scope_code,
            placement: None,
            target_group_record_index,
            asset,
        })
    }
    pub(crate) fn scope_record_index(&self) -> u32 {
        self.scope.value
    }
    pub(crate) fn scope_byte_offset(&self) -> u64 {
        self.scope.offset
    }
    #[cfg(test)]
    pub(crate) fn layout(&self) -> DesignDecalScopeLayout {
        self.layout
    }
    fn asset_reference_offset(&self) -> u64 {
        self.scope.offset + u64_from_index(self.layout.asset_reference()) + 1
    }
    fn scope_code_offset(&self) -> u64 {
        self.scope.offset + u64_from_index(self.layout.scope_code())
    }
    fn target_group_reference_offset(&self) -> u64 {
        self.scope.offset + u64_from_index(self.layout.target_group_reference()) + 1
    }
}

/// Exact image and target binding owned by one Design `Decal` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignDecalImageWire {
    /// Globally unique deterministic identifier for this native binding.
    id: String,
    /// Decal scope record index.
    scope_record_index: u32,
    scope_layout: DesignDecalScopeLayout,
    /// Byte offset of the scope's marked image-asset reference.
    asset_reference_offset: u64,
    /// Scope code byte between the image-asset and target-group references.
    scope_code: u8,
    /// Byte offset of `scope_code`.
    scope_code_offset: u64,
    /// Target construction-group record index.
    target_group_record_index: u32,
    /// Byte offset of the scope's marked target-group reference.
    target_group_reference_offset: u64,
    /// Dynamic class tag of the primary image-asset record.
    asset_class_tag: String,
    /// Primary image-asset record index.
    asset_record_index: u32,
    /// Byte offset of the primary image-asset record.
    asset_byte_offset: u64,
    /// Byte length from the primary image header to the name-record header.
    asset_frame_length: u64,
    /// Design entity suffix carried by the primary image record.
    asset_entity_suffix: u32,
    /// Byte offset of the marked Design entity-suffix reference.
    asset_entity_reference_offset: u64,
    /// Dynamic class tag of the image-name record.
    name_class_tag: String,
    /// Image-name record index.
    name_record_index: u32,
    /// Byte offset of the image-name record.
    name_byte_offset: u64,
    /// Byte length of the complete image-name record.
    name_frame_length: u64,
    /// Archive entry basename stored by the image-name record.
    asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    asset_name_offset: u64,
    /// Frame of the Decal placement record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_placement"
    )]
    placement: Option<DesignDecalPlacement>,
}

#[derive(Serialize)]
struct DesignDecalImageWireRef<'a> {
    id: &'a str,
    scope_record_index: u32,
    scope_layout: DesignDecalScopeLayout,
    asset_reference_offset: u64,
    scope_code: u8,
    scope_code_offset: u64,
    target_group_record_index: u32,
    target_group_reference_offset: u64,
    asset_class_tag: &'a str,
    asset_record_index: u32,
    asset_byte_offset: u64,
    asset_frame_length: u64,
    asset_entity_suffix: u32,
    asset_entity_reference_offset: u64,
    name_class_tag: &'a str,
    name_record_index: u32,
    name_byte_offset: u64,
    name_frame_length: u64,
    asset_name: &'a str,
    asset_name_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    placement: Option<&'a DesignDecalPlacement>,
}

impl Serialize for DesignDecalImage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let [asset_class_tag, name_class_tag] = &self.asset.class_tags;
        DesignDecalImageWireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index(),
            scope_layout: self.layout,
            asset_reference_offset: self.asset_reference_offset(),
            scope_code: self.scope_code,
            scope_code_offset: self.scope_code_offset(),
            target_group_record_index: self.target_group_record_index,
            target_group_reference_offset: self.target_group_reference_offset(),
            asset_class_tag,
            asset_record_index: self.asset.record_index,
            asset_byte_offset: self.asset.byte_offset,
            asset_frame_length: self.asset.primary_frame_length,
            asset_entity_suffix: self.asset.entity_suffix,
            asset_entity_reference_offset: self.asset.entity_reference_offset(),
            name_class_tag,
            name_record_index: self.asset.name_record_index(),
            name_byte_offset: self.asset.name_byte_offset(),
            name_frame_length: self.asset.name_frame_length(),
            asset_name: &self.asset.name,
            asset_name_offset: self.asset.name_offset(),
            placement: self.placement.as_ref(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignDecalImageWire> for DesignDecalImage {
    type Error = String;
    fn try_from(wire: DesignDecalImageWire) -> Result<Self, Self::Error> {
        let scope_offset = wire
            .asset_reference_offset
            .checked_sub(u64_from_index(wire.scope_layout.asset_reference()) + 1)
            .ok_or("asset_reference_offset precedes the Decal scope prefix")?;
        let asset = DesignDecalAsset::new(
            wire.scope_layout,
            [wire.asset_class_tag, wire.name_class_tag],
            [wire.asset_record_index, wire.name_record_index],
            wire.asset_byte_offset,
            wire.asset_entity_suffix,
            wire.asset_name,
        )?;
        let mut image = Self::new(
            wire.id,
            Located {
                value: wire.scope_record_index,
                offset: scope_offset,
            },
            wire.scope_layout,
            wire.scope_code,
            wire.target_group_record_index,
            asset,
        )?;
        for (name, declared, derived) in [
            (
                "scope_code_offset",
                wire.scope_code_offset,
                image.scope_code_offset(),
            ),
            (
                "target_group_reference_offset",
                wire.target_group_reference_offset,
                image.target_group_reference_offset(),
            ),
            (
                "asset_frame_length",
                wire.asset_frame_length,
                image.asset.primary_frame_length,
            ),
            (
                "asset_entity_reference_offset",
                wire.asset_entity_reference_offset,
                image.asset.entity_reference_offset(),
            ),
            (
                "name_byte_offset",
                wire.name_byte_offset,
                image.asset.name_byte_offset(),
            ),
            (
                "name_frame_length",
                wire.name_frame_length,
                image.asset.name_frame_length(),
            ),
            (
                "asset_name_offset",
                wire.asset_name_offset,
                image.asset.name_offset(),
            ),
        ] {
            if declared != derived {
                return Err(format!("{name} must match the Decal record layout"));
            }
        }
        if let Some(placement) = &wire.placement {
            placement.admit(image.asset.byte_offset())?;
        }
        image.placement = wire.placement;
        Ok(image)
    }
}

#[cfg(test)]
impl From<DesignDecalImage> for DesignDecalImageWire {
    fn from(value: DesignDecalImage) -> Self {
        let scope_record_index = value.scope_record_index();
        let asset_reference_offset = value.asset_reference_offset();
        let scope_code_offset = value.scope_code_offset();
        let target_group_reference_offset = value.target_group_reference_offset();
        let asset_record_index = value.asset.record_index;
        let asset_byte_offset = value.asset.byte_offset;
        let asset_frame_length = value.asset.primary_frame_length;
        let asset_entity_suffix = value.asset.entity_suffix;
        let asset_entity_reference_offset = value.asset.entity_reference_offset();
        let name_record_index = value.asset.name_record_index();
        let name_byte_offset = value.asset.name_byte_offset();
        let name_frame_length = value.asset.name_frame_length();
        let asset_name_offset = value.asset.name_offset();
        let [asset_class_tag, name_class_tag] = value.asset.class_tags;
        Self {
            id: value.id,
            scope_record_index,
            scope_layout: value.layout,
            asset_reference_offset,
            scope_code: value.scope_code,
            scope_code_offset,
            target_group_record_index: value.target_group_record_index,
            target_group_reference_offset,
            asset_class_tag,
            asset_record_index,
            asset_byte_offset,
            asset_frame_length,
            asset_entity_suffix,
            asset_entity_reference_offset,
            name_class_tag,
            name_record_index,
            name_byte_offset,
            name_frame_length,
            asset_name: value.asset.name,
            asset_name_offset,
            placement: value.placement,
        }
    }
}

/// One indexed record header in the recursive Design `BulkStream` tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignRecordHeader {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Index of this record within the recursive `BulkStream` tree.
    pub(crate) record_index: u32,
    /// Source per-file dynamic three-digit ASCII class tag naming this record's type.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of this header within its Design `BulkStream`.
    pub(crate) byte_offset: u64,
}

#[cfg(test)]
mod tests;
