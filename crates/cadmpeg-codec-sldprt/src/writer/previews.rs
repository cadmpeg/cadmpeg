// SPDX-License-Identifier: Apache-2.0
//! Preview payloads authored from embedded neutral assets.

use cadmpeg_core::decode::View;
use cadmpeg_ir::assets::{Asset, AssetContent};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::container::PayloadFamily;
use crate::loss::SldprtLossCode;

pub(super) fn sections(
    ir: &CadIr,
) -> Result<Vec<(String, Vec<u8>)>, super::write_error::WriteError> {
    ir.model
        .assets
        .iter()
        .enumerate()
        .map(|(ordinal, asset)| {
            payload(asset).map(|payload| (format!("Contents/Preview-{ordinal}"), payload))
        })
        .collect()
}

pub(super) fn reduce(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), super::write_error::WriteError> {
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
            Err(super::write_error::WriteError::Decline(reason)) => losses.push(
                SldprtLossCode::WriterEntityOmitted.note(format!("asset {}: {reason}", asset.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    ir.model.assets = assets;
    Ok(())
}

fn payload(asset: &Asset) -> Result<Vec<u8>, super::write_error::WriteError> {
    let AssetContent::Embedded { data } = &asset.content else {
        return Err(super::write_error::WriteError::Decline(
            "SLDPRT previews require embedded image content".into(),
        ));
    };
    let bytes = data.as_bytes();
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Ok(bytes.to_vec());
    }
    if bytes.starts_with(b"BM") && View::u32_le_at(bytes, 14) == Some(40) {
        let dib = bytes.get(14..).ok_or_else(|| {
            super::write_error::WriteError::Decline("SLDPRT bitmap has no DIB body".into())
        })?;
        let size = thumbnail_size(dib.len())?;
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
    Err(super::write_error::WriteError::Decline(
        "asset has no PNG or BITMAPINFOHEADER thumbnail encoding".into(),
    ))
}

fn thumbnail_size(dib_length: usize) -> Result<u32, super::write_error::WriteError> {
    let value = cadmpeg_core::decode::u64_from_index(dib_length)
        .checked_add(4)
        .ok_or(super::write_error::WriteError::Capacity {
            field: &super::CAPACITY_FIELDS[5],
            value: u64::MAX,
        })?;
    u32::try_from(value).map_err(|_| super::write_error::WriteError::Capacity {
        field: &super::CAPACITY_FIELDS[5],
        value,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::assets::{Asset, AssetContent, AssetData, AssetId};

    #[test]
    fn preview_payload_accepts_png_and_declines_other_bytes() {
        let make_asset = |bytes: Vec<u8>| {
            Asset::try_new(
                AssetId::mint("synthetic:test:asset#preview").unwrap(),
                None,
                None,
                AssetContent::Embedded {
                    data: AssetData::new(bytes).unwrap(),
                },
                None,
            )
            .unwrap()
        };
        let png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        assert_eq!(super::payload(&make_asset(png.clone())).unwrap(), png);
        assert!(matches!(
            super::payload(&make_asset(vec![1])),
            Err(super::super::write_error::WriteError::Decline(_))
        ));
    }

    #[test]
    fn thumbnail_size_reports_native_field_capacity() {
        let limit = u32::MAX as usize;
        assert_eq!(super::thumbnail_size(limit - 4).unwrap(), u32::MAX);
        let error: cadmpeg_ir::codec::write::EncodeError =
            super::thumbnail_size(limit - 3).unwrap_err().into();
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field, limit: actual_limit, value
        } if field.field == "thumbnail.byte_length" && actual_limit == u64::from(u32::MAX) && value == actual_limit + 1)
        );
        let error: cadmpeg_ir::codec::write::EncodeError =
            super::thumbnail_size(usize::MAX).unwrap_err().into();
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field, limit: actual_limit, value
        } if field.field == "thumbnail.byte_length" && actual_limit == u64::from(u32::MAX) && value >= actual_limit)
        );
    }
}
