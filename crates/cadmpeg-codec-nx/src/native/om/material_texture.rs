// SPDX-License-Identifier: Apache-2.0
//! TIFF material assets with checked paths and image-directory bounds.

use cadmpeg_core::decode::View;
use cadmpeg_ir::hash::sha256_hex;
use serde::{Deserialize, Serialize};

use crate::container::Container;

const TEXTURE_PREFIX: &str = "/Root/materialsTif/";
const ROOT_PREFIX: &str = "/Root/";
const TIFF_VERSION: u16 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TiffByteOrder {
    LittleEndian,
    BigEndian,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TextureWire", into = "TextureWire")]
pub(crate) struct MaterialTextureAsset {
    pub(crate) id: String,
    pub(crate) byte_order: TiffByteOrder,
    first_ifd_offset: u32,
    byte_len: u64,
    pub(crate) sha256: String,
    source_entry: String,
    pub(crate) source_offset: u64,
}

impl MaterialTextureAsset {
    fn new(
        id: String,
        byte_order: TiffByteOrder,
        first_ifd_offset: u32,
        byte_len: u64,
        sha256: String,
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

    pub(crate) fn name(&self) -> &str {
        &self.source_entry()[TEXTURE_PREFIX.len()..]
    }

    pub(crate) fn storage_path(&self) -> &str {
        &self.source_entry()[ROOT_PREFIX.len()..]
    }

    pub(crate) fn source_entry(&self) -> &str {
        &self.source_entry
    }

    pub(crate) fn first_ifd_offset(&self) -> u32 {
        self.first_ifd_offset
    }

    pub(crate) fn byte_len(&self) -> u64 {
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
    sha256: String,
    source_entry: String,
    source_offset: u64,
}

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

pub(crate) fn material_texture_assets(container: &Container) -> Vec<MaterialTextureAsset> {
    let mut entries = container
        .entries
        .iter()
        .filter(|entry| entry.name.starts_with(TEXTURE_PREFIX))
        .collect::<Vec<_>>();
    entries.sort_by(|first, second| first.name.cmp(&second.name));
    let mut assets = Vec::new();
    for entry in entries {
        let parse = || {
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
            MaterialTextureAsset::new(
                format!("nx:container:material-texture#{}", assets.len()),
                byte_order,
                first_ifd_offset,
                size as u64,
                sha256_hex(payload),
                entry.name.clone(),
                offset,
            )
            .ok()
        };
        if let Some(asset) = parse() {
            assets.push(asset);
        }
    }
    assets
}

#[cfg(test)]
mod tests {
    use super::MaterialTextureAsset;

    #[test]
    fn wire_keeps_derived_name_version_and_field_order() {
        let json = r#"{"id":"texture","name":"Steel","byte_order":"little_endian","version":42,"first_ifd_offset":8,"byte_len":10,"sha256":"hash","source_entry":"/Root/materialsTif/Steel","source_offset":20}"#;
        let value: MaterialTextureAsset = serde_json::from_str(json).unwrap();
        assert_eq!(value.storage_path(), "materialsTif/Steel");
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
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
}
