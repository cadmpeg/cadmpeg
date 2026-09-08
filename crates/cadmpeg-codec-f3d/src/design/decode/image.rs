// SPDX-License-Identifier: Apache-2.0
//! Transfer uniquely named Design image resources into neutral assets.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};

pub(crate) fn embedded_image_asset(
    scan: &ContainerScan,
    asset_name: &str,
) -> Result<Option<Asset>, CodecError> {
    let mut entries = scan.entries.iter().filter(|entry| {
        scan.is_design_asset_entry(entry, ContainerRole::Image)
            && entry.name.rsplit('/').next() == Some(asset_name)
    });
    let (Some(entry), None) = (entries.next(), entries.next()) else {
        return Ok(None);
    };
    let media_type = std::path::Path::new(asset_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some("image/jpeg"),
            "png" => Some("image/png"),
            _ => None,
        })
        .map(str::to_owned);
    Ok(Some(
        Asset::try_new(
            crate::ids::neutral_asset_id(&entry.name),
            Some(asset_name.to_owned()),
            media_type,
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(scan.entry_bytes(&entry.name)?.to_vec())
                    .ok_or_else(|| {
                    CodecError::Malformed("asset data must not be empty".into())
                })?,
            },
            Some(crate::ids::native_scope(&entry.name)),
        )
        .map_err(CodecError::Malformed)?,
    ))
}
