// SPDX-License-Identifier: Apache-2.0
//! TIFF material assets with checked paths and image-directory bounds.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

use crate::container::Container;

const TEXTURE_PREFIX: &str = "/Root/materialsTif/";
const ROOT_PREFIX: &str = "/Root/";
const TIFF_VERSION: u16 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::native::om) enum TiffByteOrder {
    LittleEndian,
    BigEndian,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "TextureWire")]
pub(in crate::native) struct MaterialTextureAsset {
    pub(in crate::native) id: String,
    pub(super) byte_order: TiffByteOrder,
    first_ifd_offset: u32,
    byte_len: u64,
    pub(in crate::native) sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    source_entry: String,
    pub(in crate::native) source_offset: u64,
}

#[derive(Serialize)]
struct TextureRef<'a> {
    id: &'a str,
    name: &'a str,
    byte_order: TiffByteOrder,
    version: u16,
    first_ifd_offset: u32,
    byte_len: u64,
    sha256: &'a cadmpeg_ir::hash::digest::Sha256Digest,
    source_entry: &'a str,
    source_offset: u64,
}

impl Serialize for MaterialTextureAsset {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TextureRef {
            id: &self.id,
            name: self.name(),
            byte_order: self.byte_order,
            version: TIFF_VERSION,
            first_ifd_offset: self.first_ifd_offset,
            byte_len: self.byte_len,
            sha256: &self.sha256,
            source_entry: &self.source_entry,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

impl MaterialTextureAsset {
    fn new(
        id: String,
        byte_order: TiffByteOrder,
        first_ifd_offset: u32,
        byte_len: u64,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest,
        source_entry: String,
        source_offset: u64,
    ) -> Result<Self, &'static str> {
        if source_entry
            .strip_prefix(TEXTURE_PREFIX)
            .is_none_or(str::is_empty)
        {
            return Err("source_entry: requires a nonempty materialsTif path");
        }
        if first_ifd_offset < 8 || u64::from(first_ifd_offset) >= byte_len {
            return Err("first_ifd_offset: must follow the TIFF header and be within byte_len");
        }
        Ok(Self {
            id,
            byte_order,
            first_ifd_offset,
            byte_len,
            sha256,
            source_entry,
            source_offset,
        })
    }

    pub(in crate::native) fn name(&self) -> &str {
        &self.source_entry()[TEXTURE_PREFIX.len()..]
    }

    pub(super) fn storage_path(&self) -> &str {
        &self.source_entry()[ROOT_PREFIX.len()..]
    }

    pub(super) fn source_entry(&self) -> &str {
        &self.source_entry
    }

    #[cfg(test)]
    pub(super) fn first_ifd_offset(&self) -> u32 {
        self.first_ifd_offset
    }

    pub(in crate::native) fn byte_len(&self) -> u64 {
        self.byte_len
    }
}

#[derive(Serialize, Deserialize)]
struct TextureWire {
    id: String,
    name: String,
    byte_order: TiffByteOrder,
    version: u16,
    first_ifd_offset: u32,
    byte_len: u64,
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    source_entry: String,
    source_offset: u64,
}

#[cfg(test)]
impl From<MaterialTextureAsset> for TextureWire {
    fn from(value: MaterialTextureAsset) -> Self {
        Self {
            name: value.name().to_owned(),
            version: TIFF_VERSION,
            first_ifd_offset: value.first_ifd_offset(),
            byte_len: value.byte_len(),
            id: value.id,
            byte_order: value.byte_order,
            sha256: value.sha256,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<TextureWire> for MaterialTextureAsset {
    type Error = &'static str;

    fn try_from(wire: TextureWire) -> Result<Self, Self::Error> {
        if wire.version != TIFF_VERSION {
            return Err("version: expected TIFF version 42");
        }
        let value = Self::new(
            wire.id,
            wire.byte_order,
            wire.first_ifd_offset,
            wire.byte_len,
            wire.sha256,
            wire.source_entry,
            wire.source_offset,
        )?;
        if value.name() != wire.name {
            return Err("name: must equal the source_entry suffix");
        }
        Ok(value)
    }
}

pub(in crate::native) fn material_texture_assets(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<MaterialTextureAsset>, CodecError> {
    let name_bytes = container
        .entries
        .iter()
        .try_fold(0usize, |total, entry| total.checked_add(entry.name.len()))
        .and_then(|total| total.checked_mul(2))
        .ok_or_else(|| ctx.refuse_codec_limit("NX material texture entry scan", 0, 1))?;
    ctx.charge_work(
        u64_from_index(name_bytes),
        "scan NX material texture entries",
    )?;
    let count = container
        .entries
        .iter()
        .filter(|entry| entry.name.starts_with(TEXTURE_PREFIX))
        .count();
    let entry_bytes = count
        .checked_mul(std::mem::size_of::<&crate::container::DirEntry>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX material texture entry slots", 0, 1))?;
    let _entry_reservation = ctx.reserve_scoped(
        u64_from_index(entry_bytes),
        "sort NX material texture entries",
    )?;
    let mut entries = Vec::new();
    cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
        &mut entries,
        count,
        "allocate NX material texture entries",
    )?;
    entries.extend(
        container
            .entries
            .iter()
            .filter(|entry| entry.name.starts_with(TEXTURE_PREFIX)),
    );
    let sort_work = count
        .checked_mul(count)
        .ok_or_else(|| ctx.refuse_codec_limit("NX material texture sort work", 0, 1))?;
    ctx.charge_work(
        u64_from_index(sort_work),
        "sort NX material texture entries",
    )?;
    entries.sort_by(|first, second| first.name.cmp(&second.name));
    let mut assets = Vec::new();
    for entry in entries {
        let parsed = (|| {
            let (offset, size) = entry.file_span()?;
            let (start, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container.data.get(start..start.checked_add(size)?)?;
            let (byte_order, first_ifd_offset) = match payload.get(..8)? {
                [b'I', b'I', 42, 0, ..] => {
                    (TiffByteOrder::LittleEndian, View::u32_le_at(payload, 4)?)
                }
                [b'M', b'M', 0, 42, ..] => (TiffByteOrder::BigEndian, View::u32_be_at(payload, 4)?),
                _ => return None,
            };
            (u64::from(first_ifd_offset) < u64_from_index(size)
                && first_ifd_offset >= 8
                && entry.name.len() > TEXTURE_PREFIX.len())
            .then_some((offset, size, payload, byte_order, first_ifd_offset))
        })();
        let Some((offset, size, payload, byte_order, first_ifd_offset)) = parsed else {
            continue;
        };
        ctx.reserve_retained_vec(&mut assets, 1, "NX material texture assets")?;
        let ordinal = assets.len();
        let mut digits = 1;
        let mut value = ordinal;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        let id_len = "nx:container:material-texture#"
            .len()
            .checked_add(digits)
            .ok_or_else(|| ctx.refuse_codec_limit("NX material texture identity length", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "retain NX material texture identity")?;
        write!(id, "nx:container:material-texture#{ordinal}")
            .map_err(|_| ctx.refuse_codec_limit("write NX material texture identity", 0, 1))?;
        let mut source_entry =
            ctx.retained_string(entry.name.len(), "retain NX material texture source entry")?;
        source_entry.push_str(&entry.name);
        ctx.charge_retained(64, "retain NX material texture digest")?;
        ctx.charge_work(
            u64_from_index(payload.len()),
            "hash NX material texture payload",
        )?;
        let asset = MaterialTextureAsset::new(
            id,
            byte_order,
            first_ifd_offset,
            u64_from_index(size),
            cadmpeg_ir::hash::digest::Sha256Digest::digest(payload),
            source_entry,
            offset,
        )
        .map_err(|error| CodecError::InvalidInput(error.to_owned()))?;
        assets.push(asset);
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::{material_texture_assets, MaterialTextureAsset};
    use crate::container::{Container, DirEntry, DirEntryBody, Region};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::borrow::Cow;
    use std::sync::OnceLock;

    const TIFF: &[u8] = &[b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];

    fn container() -> Container<'static> {
        Container {
            data: Cow::Borrowed(TIFF),
            physical_size: TIFF.len() as u64,
            layout: crate::container::test_modern_layout(6),
            entries: vec![DirEntry {
                name: "/Root/materialsTif/Steel".to_owned(),
                region: Region::Header,
                body: DirEntryBody::File {
                    offset: 0,
                    len: TIFF.len() as u64,
                },
            }],
            fastload_table: None,
            indexed_section_layouts: OnceLock::new(),
            om_section_cache: OnceLock::new(),
        }
    }

    fn assert_limit(configure: impl FnOnce(&mut DecodePolicy), dimension: ResourceDimension) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        configure(&mut policy);
        let (ctx, _) = DecodeContext::from_root_bytes(TIFF, &arena, &policy).unwrap();
        let error = material_texture_assets(&ctx, &container()).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }

    #[test]
    fn material_texture_assets_refuse_collection_limit() {
        assert_limit(
            |policy| policy.limits.max_collection_items = 0,
            ResourceDimension::CollectionItems,
        );
    }

    #[test]
    fn material_texture_assets_refuse_retained_limit() {
        assert_limit(
            |policy| policy.limits.max_retained_bytes = 0,
            ResourceDimension::RetainedBytes,
        );
    }

    #[test]
    fn material_texture_assets_refuse_scoped_limit() {
        assert_limit(
            |policy| policy.limits.max_materialized_bytes = 0,
            ResourceDimension::MaterializedBytes,
        );
    }

    #[test]
    fn material_texture_assets_refuse_work_limit() {
        assert_limit(
            |policy| policy.limits.max_work_units = 0,
            ResourceDimension::WorkUnits,
        );
    }

    #[test]
    fn material_texture_assets_preserve_identity_and_source() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(TIFF, &arena, &policy).unwrap();
        let assets = material_texture_assets(&ctx, &container()).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].id, "nx:container:material-texture#0");
        assert_eq!(assets[0].name(), "Steel");
        assert_eq!(assets[0].source_entry(), "/Root/materialsTif/Steel");
    }

    #[test]
    fn wire_keeps_derived_name_version_and_field_order() {
        let json = r#"{"id":"texture","name":"Steel","byte_order":"little_endian","version":42,"first_ifd_offset":8,"byte_len":10,"sha256":"d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa","source_entry":"/Root/materialsTif/Steel","source_offset":20}"#;
        let value: MaterialTextureAsset = serde_json::from_str(json).unwrap();
        assert_eq!(value.storage_path(), "materialsTif/Steel");
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&super::TextureWire::from(value.clone())).unwrap()
        );
        for (field, invalid) in [
            ("name", serde_json::json!("Other")),
            ("version", serde_json::json!(43)),
            ("first_ifd_offset", serde_json::json!(7)),
            ("first_ifd_offset", serde_json::json!(10)),
            ("byte_len", serde_json::json!(8)),
            ("source_entry", serde_json::json!("/Root/materialsTif/")),
            ("source_entry", serde_json::json!("/Root/Other/Steel")),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = invalid;
            assert!(serde_json::from_value::<MaterialTextureAsset>(wire).is_err());
        }
    }

    #[test]
    fn material_texture_native_limit_refuses_before_name_copy() {
        let wire = serde_json::json!({
            "id": "nx:container:material-texture#0", "name": "Steel",
            "byte_order": "little_endian", "version": 42,
            "first_ifd_offset": 8, "byte_len": 10,
            "sha256": "d04b98f48e8f8bcc15c6ae5ac050801cd6dcfd428fb5f9e65c4e16e7807340fa",
            "source_entry": "/Root/materialsTif/Steel", "source_offset": 20
        });
        let value: MaterialTextureAsset = serde_json::from_value(wire.clone()).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(&value, wire);
    }
}
