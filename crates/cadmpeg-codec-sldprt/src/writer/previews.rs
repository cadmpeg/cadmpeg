// SPDX-License-Identifier: Apache-2.0
//! Preview payloads authored from embedded neutral assets.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::container::PayloadFamily;
use crate::loss::SldprtLossCode;

pub(super) fn sections(ir: &CadIr) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
    ir.model
        .assets
        .iter()
        .enumerate()
        .map(|(ordinal, asset)| {
            payload(asset).map(|payload| (format!("Contents/Preview-{ordinal}"), payload))
        })
        .collect()
}

pub(super) fn reduce(ir: &mut CadIr, losses: &mut Vec<LossNote>) -> Result<(), CodecError> {
    let mut assets = Vec::new();
    for asset in std::mem::take(&mut ir.model.assets) {
        match payload(&asset) {
            Ok(payload) => {
                let expected = if matches!(
                    crate::container::payload_family(&payload),
                    PayloadFamily::PngPreview
                ) {
                    "preview.png"
                } else {
                    "thumbnail.bmp"
                };
                if asset
                    .name
                    .as_ref()
                    .is_some_and(|name| name.as_str() != expected)
                {
                    losses.push(SldprtLossCode::WriterFieldsOmitted.note(format!(
                        "asset {}: the preview record has no independent resource-name field",
                        asset.id
                    )));
                }
                assets.push(asset);
            }
            Err(CodecError::UnsupportedContent(reason)) => losses.push(
                SldprtLossCode::WriterEntityOmitted.note(format!("asset {}: {reason}", asset.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    ir.model.assets = assets;
    Ok(())
}

fn payload(asset: &Asset) -> Result<Vec<u8>, CodecError> {
    let AssetContent::Embedded { data } = &asset.content else {
        return Err(CodecError::UnsupportedContent(
            "SLDPRT previews require embedded image content".into(),
        ));
    };
    let bytes = data.as_bytes();
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Ok(bytes.to_vec());
    }
    if bytes.starts_with(b"BM") && View::u32_le_at(bytes, 14) == Some(40) {
        let dib = bytes.get(14..).ok_or_else(|| {
            CodecError::UnsupportedContent("SLDPRT bitmap has no DIB body".into())
        })?;
        // (O-capacity): native thumbnail byte size has a u32 field.
        let size = dib
            .len()
            .checked_add(4)
            .and_then(|size| u32::try_from(size).ok())
            .ok_or_else(|| {
                CodecError::malformed("SLDPRT thumbnail exceeds its u32 byte-size field")
            })?;
        let mut out = Vec::new();
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(dib);
        if matches!(
            crate::container::payload_family(&out),
            PayloadFamily::BmpThumbnail
        ) {
            return Ok(out);
        }
    }
    Err(CodecError::UnsupportedContent(
        "asset has no PNG or BITMAPINFOHEADER thumbnail encoding".into(),
    ))
}
