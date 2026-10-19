// SPDX-License-Identifier: Apache-2.0
//! Embedded resources are authored from the current asset bytes.

use super::*;
use cadmpeg_ir::assets::AssetContent;

const EMBEDDED_BITMAP_CLASS: [u8; 16] = [
    0xc1, 0x6f, 0x2e, 0x77, 0x7b, 0xb1, 0xc4, 0x4f, 0x8f, 0x54, 0x5f, 0xda, 0x51, 0x1d, 0x76, 0xd2,
];

pub(in crate::writer) fn images(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for asset in &ir.model.assets {
        let AssetContent::Embedded { data } = &asset.content else {
            continue;
        };
        let data = data.as_bytes();
        let native = asset
            .native_ref
            .as_deref()
            .and_then(|id| {
                records(ir, "embedded_images")
                    .iter()
                    .find(|record| record.id() == id)
            })
            .map(cadmpeg_ir::NativeRecord::fields)
            .transpose()?;
        let fields = native.as_ref();
        let mut payload = vec![0x11];
        payload.extend(utf16(
            fields
                .and_then(|fields| fields.get("file_path"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?);
        payload.extend(crc32fast::hash(data).to_le_bytes());
        payload.extend(0i32.to_le_bytes());
        payload.extend(
            u32::try_from(data.len())
                .map_err(|_| {
                    native_count_limit(
                        u32::MAX.into(),
                        cadmpeg_core::decode::u64_from_index(data.len()),
                    )
                })?
                .to_le_bytes(),
        );
        payload.extend(data);
        payload.extend(uuid(
            fields.and_then(|fields| fields.get("source_uuid")),
            asset.id.as_str(),
        ));
        payload.extend(utf16(asset.name.as_ref().map_or("", |name| name.as_str()))?);
        output.push(crc_chunk_with_direct(
            0x2000_8090,
            &class_wrapper(EMBEDDED_BITMAP_CLASS, &payload)?,
            &[],
        )?);
    }
    Ok(output)
}
