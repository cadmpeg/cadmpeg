// SPDX-License-Identifier: Apache-2.0
//! Embedded resources are authored from the current asset bytes.

use super::{class_wrapper, crc_chunk_with_direct, records, utf16, uuid, CadIr, CodecError, Value};
use crate::loss::RhinoLossCode;
use cadmpeg_ir::assets::AssetContent;
use cadmpeg_ir::report::loss::LossNote;

const EMBEDDED_BITMAP_CLASS: [u8; 16] = [
    0xc1, 0x6f, 0x2e, 0x77, 0x7b, 0xb1, 0xc4, 0x4f, 0x8f, 0x54, 0x5f, 0xda, 0x51, 0x1d, 0x76, 0xd2,
];

/// Emitted bitmap-table records with the native images they carry.
pub(in crate::writer) struct ImageExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) losses: Vec<LossNote>,
    /// Held `embedded_images` records re-emitted from their linked asset bytes.
    pub(in crate::writer) regenerated: std::collections::BTreeSet<String>,
}

fn image_length(identity: &str, native: Option<&str>, length: u64) -> Result<u32, Box<LossNote>> {
    u32::try_from(length).map_err(|_| {
        Box::new(RhinoLossCode::WriterImageOmitted.note(format!(
            "asset {identity} native {} omitted: image length {length} exceeds the 3DM field",
            native.unwrap_or("unlinked")
        )))
    })
}

pub(in crate::writer) fn images(ir: &CadIr) -> Result<ImageExport, CodecError> {
    let mut output = ImageExport {
        records: Vec::new(),
        losses: Vec::new(),
        regenerated: std::collections::BTreeSet::new(),
    };
    for asset in &ir.model.assets {
        if asset.native_ref.as_deref().is_some_and(|id| {
            records(ir, "previews")
                .iter()
                .any(|record| record.id() == id)
        }) {
            continue;
        }
        let AssetContent::Embedded { data } = &asset.content else {
            output
                .losses
                .push(RhinoLossCode::WriterImageOmitted.note(format!(
                    "asset {} has no embedded content and is not written to the bitmap table",
                    asset.id
                )));
            continue;
        };
        let data = data.as_bytes();
        let length = match image_length(
            asset.id.as_str(),
            asset.native_ref.as_deref(),
            cadmpeg_core::decode::u64_from_index(data.len()),
        ) {
            Ok(length) => length,
            Err(loss) => {
                output.losses.push(*loss);
                continue;
            }
        };
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
        payload.extend(length.to_le_bytes());
        payload.extend(data);
        payload.extend(uuid(
            fields.and_then(|fields| fields.get("source_uuid")),
            asset.id.as_str(),
        ));
        payload.extend(utf16(asset.name.as_ref().map_or("", |name| name.as_str()))?);
        output.records.push(crc_chunk_with_direct(
            0x2000_8090,
            &class_wrapper(EMBEDDED_BITMAP_CLASS, &payload)?,
            &[],
        )?);
        if let Some(id) = asset.native_ref.as_deref() {
            if records(ir, "embedded_images")
                .iter()
                .any(|record| record.id() == id)
            {
                output.regenerated.insert(id.to_owned());
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrepresentable_image_length_names_both_records() {
        let length = u64::from(u32::MAX) + 1;
        let loss = image_length(
            "test:model:asset#image",
            Some("rhino:asset:image#native"),
            length,
        )
        .unwrap_err();
        assert_eq!(loss.code, RhinoLossCode::WriterImageOmitted.kind());
        assert!(loss.message.contains("test:model:asset#image"));
        assert!(loss.message.contains("rhino:asset:image#native"));
    }
}
