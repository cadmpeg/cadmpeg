// SPDX-License-Identifier: Apache-2.0
//! Decode image resources into typed assets before discarding source layout.

use super::{EmbeddedImageCompression, EmbeddedImageRecord, WindowsBitmapRecord};
use crate::chunks::{chunk_at, BoundedReader, FramingError};
use crate::container::Scan;
use crate::loss::RhinoLossCode;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent, AssetData, AssetId};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

fn framing(error: FramingError) -> CodecError {
    match error {
        FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
        error => CodecError::malformed(error),
    }
}

fn buffer(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &Scan<'_>,
    reader: &mut BoundedReader<'_>,
    compressed: bool,
) -> Result<Vec<u8>, CodecError> {
    let count = reader.u32().map_err(framing)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let count = cadmpeg_core::decode::index_from_u32(count);
    let (crc, method) = if compressed {
        (
            Some(reader.u32().map_err(framing)?),
            reader.u8().map_err(framing)?,
        )
    } else {
        (None, 0)
    };
    let bytes = match method {
        0 => {
            let bytes = reader.take(count).map_err(framing)?;
            ctx.retain_bytes(u64_from_index(bytes.len()), "Rhino image asset bytes")?;
            bytes.to_vec()
        }
        1 => {
            let chunk = chunk_at(
                scan.data,
                reader.position(),
                reader.end(),
                scan.archive,
                false,
            )
            .map_err(framing)?;
            if chunk.typecode != 0x4000_8000 || chunk.short() {
                return Err(CodecError::malformed(
                    "Rhino image buffer has no anonymous compressed chunk",
                ));
            }
            let source = root
                .child(chunk.body().start, chunk.body().end)
                .ok_or_else(|| {
                    CodecError::malformed("Rhino image compressed bytes escape the source view")
                })?;
            if source.window() != &scan.data[chunk.body()] {
                return Err(CodecError::malformed(
                    "Rhino image view does not alias its source record",
                ));
            }
            let (bytes, consumed) = cadmpeg_container::compression::inflate_zlib_member_owned(
                ctx,
                source,
                ExpandSpec::Exact(u64_from_index(count)),
            )?;
            if consumed != chunk.body().len() {
                return Err(CodecError::malformed(
                    "Rhino image compressed chunk contains trailing bytes",
                ));
            }
            reader
                .skip(chunk.next_offset() - reader.position())
                .map_err(framing)?;
            bytes
        }
        _ => {
            return Err(CodecError::malformed(
                "Rhino image buffer compression method is not supported",
            ))
        }
    };
    if crc.is_some_and(|crc| crc32fast::hash(&bytes) != crc) {
        return Err(CodecError::malformed(
            "Rhino image buffer checksum does not match its decoded bytes",
        ));
    }
    Ok(bytes)
}

fn image_bytes(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &Scan<'_>,
    image: &EmbeddedImageRecord,
) -> Result<Vec<u8>, CodecError> {
    let start = usize::try_from(image.buffer_offset).map_err(CodecError::malformed)?;
    let length = usize::try_from(image.buffer_byte_len).map_err(CodecError::malformed)?;
    let end = start
        .checked_add(length)
        .ok_or_else(|| CodecError::malformed("Rhino image source range overflows"))?;
    let mut reader = BoundedReader::new(scan.data, start, end).map_err(framing)?;
    let bytes = buffer(
        ctx,
        root,
        scan,
        &mut reader,
        image.compression_method == EmbeddedImageCompression::Compressed,
    )?;
    if reader.remaining() != 0 || u64_from_index(bytes.len()) != image.uncompressed_byte_len {
        return Err(CodecError::malformed(
            "Rhino image data does not match its admitted size",
        ));
    }
    if image.image_crc32 != 0 && crc32fast::hash(&bytes) != image.image_crc32 {
        return Err(CodecError::malformed(
            "Rhino embedded image checksum does not match its content",
        ));
    }
    Ok(bytes)
}

fn bitmap_bytes(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &Scan<'_>,
    bitmap: &WindowsBitmapRecord,
) -> Result<Vec<u8>, CodecError> {
    if bitmap.header_size != 40 {
        return Err(CodecError::malformed(
            "Rhino Windows bitmap has an unprojected extended header",
        ));
    }
    if bitmap.width_pixels <= 0 || bitmap.height_pixels == 0 || bitmap.planes != 1 {
        return Err(CodecError::malformed(
            "Rhino Windows bitmap dimensions or plane count do not describe an image",
        ));
    }
    let palette_count = if bitmap.colors_used > 0 {
        usize::try_from(bitmap.colors_used).map_err(CodecError::malformed)?
    } else {
        match bitmap.bits_per_pixel {
            1 => 2,
            4 => 16,
            8 => 256,
            _ => 0,
        }
    };
    let palette_size = palette_count
        .checked_mul(4)
        .ok_or_else(|| CodecError::malformed("Rhino bitmap palette overflows"))?;
    let image_size = usize::try_from(bitmap.image_byte_len).map_err(CodecError::malformed)?;
    let total = palette_size
        .checked_add(image_size)
        .ok_or_else(|| CodecError::malformed("Rhino bitmap byte count overflows"))?;
    let start = usize::try_from(bitmap.pixel_buffer_offset).map_err(CodecError::malformed)?;
    let length = usize::try_from(bitmap.pixel_buffer_byte_len).map_err(CodecError::malformed)?;
    let end = start
        .checked_add(length)
        .ok_or_else(|| CodecError::malformed("Rhino bitmap source range overflows"))?;
    let mut reader = BoundedReader::new(scan.data, start, end).map_err(framing)?;
    let mut pixels = if scan.archive == crate::chunks::ArchiveVersion::V1 {
        let bytes = reader.take(total).map_err(framing)?;
        ctx.retain_bytes(u64_from_index(total), "Rhino bitmap pixels")?;
        bytes.to_vec()
    } else {
        let mut bytes = buffer(ctx, root, scan, &mut reader, true)?;
        if bytes.len() == palette_size && image_size != 0 {
            bytes.extend(buffer(ctx, root, scan, &mut reader, true)?);
        }
        bytes
    };
    if pixels.len() != total || reader.remaining() != 0 {
        return Err(CodecError::malformed(
            "Rhino Windows bitmap pixels do not match the header",
        ));
    }
    let file_size = total
        .checked_add(54)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| CodecError::malformed("Rhino bitmap cannot fit a BMP file header"))?;
    let pixel_offset = palette_size
        .checked_add(54)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            CodecError::malformed("Rhino bitmap palette cannot fit a BMP file header")
        })?;
    ctx.retain_bytes(54, "Rhino bitmap asset header")?;
    let mut file = b"BM".to_vec();
    file.extend(file_size.to_le_bytes());
    file.extend([0; 4]);
    file.extend(pixel_offset.to_le_bytes());
    file.extend(40_u32.to_le_bytes());
    file.extend(bitmap.width_pixels.to_le_bytes());
    file.extend(bitmap.height_pixels.to_le_bytes());
    file.extend(bitmap.planes.to_le_bytes());
    file.extend(bitmap.bits_per_pixel.to_le_bytes());
    file.extend(bitmap.compression.to_le_bytes());
    file.extend(bitmap.image_byte_len.to_le_bytes());
    for value in bitmap.pixels_per_meter {
        file.extend(value.to_le_bytes());
    }
    file.extend(bitmap.colors_used.to_le_bytes());
    file.extend(bitmap.important_colors.to_le_bytes());
    file.append(&mut pixels);
    Ok(file)
}

fn insert(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    source: &str,
    name: &str,
    bytes: Vec<u8>,
    ordinal: usize,
) -> Result<Option<AssetId>, CodecError> {
    let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    };
    let Some(data) = AssetData::new(bytes) else {
        return Ok(None);
    };
    let id = AssetId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "model", "image-asset"),
        IdentityKey::zero_padded(u64_from_index(ordinal), 6),
    );
    ctx.charge_entities(1, "Rhino image assets")?;
    ir.model.assets.push(Asset {
        id: id.clone(),
        name: cadmpeg_core::text::NonBlankString::new(name),
        media_type: media_type.and_then(cadmpeg_core::text::NonBlankString::new),
        content: AssetContent::Embedded { data },
        native_ref: Some(source.to_owned()),
    });
    Ok(Some(id))
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &Scan<'_>,
    ir: &mut CadIr,
    images: &mut [EmbeddedImageRecord],
    bitmaps: &mut [WindowsBitmapRecord],
    losses: &mut Vec<LossNote>,
) -> Result<Vec<u64>, CodecError> {
    let mut unresolved = Vec::new();
    for (ordinal, image) in images.iter_mut().enumerate() {
        match image_bytes(ctx, root, scan, image) {
            Ok(bytes) => {
                image.asset = insert(
                    ctx,
                    ir,
                    &image.id,
                    if image.name.is_empty() {
                        &image.file_path
                    } else {
                        &image.name
                    },
                    bytes,
                    ordinal,
                )?
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{} image content is not projected: {error}",
                    image.id
                )));
                unresolved.push(image.source_offset);
            }
        }
    }
    for (ordinal, bitmap) in bitmaps.iter_mut().enumerate() {
        match bitmap_bytes(ctx, root, scan, bitmap) {
            Ok(bytes) => {
                bitmap.asset = insert(
                    ctx,
                    ir,
                    &bitmap.id,
                    &bitmap.file_path,
                    bytes,
                    images.len() + ordinal,
                )?
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{} bitmap content is not projected: {error}",
                    bitmap.id
                )));
                unresolved.push(bitmap.source_offset);
            }
        }
    }
    Ok(unresolved)
}
