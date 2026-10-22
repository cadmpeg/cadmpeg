// SPDX-License-Identifier: Apache-2.0
//! Preview resources projected as embedded document assets.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent, AssetData, AssetId};
use cadmpeg_ir::{Annotations, CadIr, Exactness};

use crate::container::{ContainerScan, PayloadFamily};

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut CadIr,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    for (ordinal, section) in scan.sections().enumerate() {
        let family = crate::container::payload_family(section.payload());
        if !matches!(
            family,
            PayloadFamily::PngPreview | PayloadFamily::BmpThumbnail
        ) {
            continue;
        }
        let id = AssetId::mint(format!("sldprt:asset:preview#{ordinal}"))
            .map_err(CodecError::malformed)?;
        let source = scan
            .section_view(section)
            .ok_or_else(|| CodecError::malformed("SLDPRT preview lacks its source view"))?;
        let asset = ctx.record(id.as_str(), source, || {
            let (name, media_type, bytes) = match family {
                PayloadFamily::PngPreview => (
                    "preview.png",
                    Some(cadmpeg_core::nonblank_literal!("image/png")),
                    super::retained_copy(
                        ctx,
                        source
                            .read_at(source.start(), source.window().len())
                            .ok_or_else(|| {
                                CodecError::malformed("SLDPRT PNG preview lies outside its source")
                            })?,
                        "project SLDPRT PNG preview",
                    )?,
                ),
                PayloadFamily::BmpThumbnail => match bitmap_file(source) {
                    Some(bytes) => (
                        "thumbnail.bmp",
                        Some(cadmpeg_core::nonblank_literal!("image/bmp")),
                        bytes,
                    ),
                    None => (
                        "thumbnail.dib",
                        None,
                        super::retained_copy(
                            ctx,
                            source
                                .read_at(source.start(), source.window().len())
                                .ok_or_else(|| {
                                    CodecError::malformed(
                                        "SLDPRT thumbnail lies outside its source",
                                    )
                                })?,
                            "project SLDPRT DIB thumbnail",
                        )?,
                    ),
                },
                _ => {
                    return Err(CodecError::malformed(
                        "SLDPRT preview changed its admitted family",
                    ))
                }
            };
            Ok(Asset {
                id: id.clone(),
                name: Some(
                    cadmpeg_core::text::NonBlankString::new(name.to_owned())
                        .ok_or_else(|| CodecError::malformed("empty preview name"))?,
                ),
                media_type,
                content: AssetContent::Embedded {
                    data: AssetData::new(bytes)
                        .ok_or_else(|| CodecError::malformed("empty SLDPRT preview"))?,
                },
                native_ref: None,
            })
        })?;
        crate::annotations::note(
            annotations,
            id.as_str(),
            section.source_stream(),
            0,
            "preview_asset",
            if family == PayloadFamily::PngPreview {
                Exactness::ByteExact
            } else {
                Exactness::Derived
            },
        )?;
        ir.model.assets.push(asset);
    }
    Ok(())
}

/// Put the typed DIB in a standard bitmap file header. Pixel bytes are content.
fn bitmap_file(source: View<'_>) -> Option<Vec<u8>> {
    let payload = source.window();
    let header = usize::try_from(View::u32_le_at(payload, 4)?).ok()?;
    if header < 40 {
        return None;
    }
    let bits = View::u16_le_at(payload, 18)?;
    let compression = View::u32_le_at(payload, 20)?;
    let declared_colors = View::u32_le_at(payload, 36)?;
    let colors = if declared_colors != 0 {
        declared_colors
    } else if bits <= 8 {
        1u32.checked_shl(u32::from(bits))?
    } else {
        0
    };
    let masks = if header == 40 {
        match compression {
            3 => 12,
            6 => 16,
            _ => 0,
        }
    } else {
        0
    };
    let pixel_offset = 14usize
        .checked_add(header)?
        .checked_add(masks)?
        .checked_add(usize::try_from(colors).ok()?.checked_mul(4)?)?;
    let source = source.child(source.start().checked_add(4)?, source.end())?;
    let dib = source.read_at(source.start(), source.window().len())?;
    let file_size = 14usize.checked_add(dib.len())?;
    if pixel_offset > file_size {
        return None;
    }
    let mut output = Vec::new();
    output.extend_from_slice(b"BM");
    output.extend_from_slice(&u32::try_from(file_size).ok()?.to_le_bytes());
    output.extend_from_slice(&[0; 4]);
    output.extend_from_slice(&u32::try_from(pixel_offset).ok()?.to_le_bytes());
    output.extend_from_slice(dib);
    Some(output)
}
