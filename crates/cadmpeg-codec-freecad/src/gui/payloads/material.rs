// SPDX-License-Identifier: Apache-2.0
//! Fixed material records followed by independent string fields of the entry.

use super::{finite32, framed_spec, spec};
use crate::native::entry_contents::{BinaryMaterial, EntryContents, MaterialSlot};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

pub(super) fn decode<'source>(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'source>,
    count: usize,
    version: u32,
    owner: &RecordOwner<'_>,
    output: &mut PartOutput,
) -> Result<(EntryContents, Vec<[Option<View<'source>>; 3]>), CodecError> {
    let mut values = crate::native::entry_contents::alloc_slots(
        ctx,
        count,
        MaterialSlot {
            fixed: None,
            image: None,
            image_path: None,
            uuid: None,
        },
        "entry material slots",
    )?;
    let mut sources = crate::native::entry_contents::alloc_slots(
        ctx,
        count,
        [None; 3],
        "material string source slots",
    )?;
    for slot in &mut values {
        let start = cursor.position();
        let length = cursor.remaining().min(24);
        let extent = cursor
            .child(
                start,
                start
                    .checked_add(length)
                    .ok_or_else(|| CodecError::malformed("material fixed extent overflows"))?,
            )
            .ok_or_else(|| CodecError::malformed("material fixed extent escapes entry"))?;
        cursor
            .skip(length)
            .ok_or_else(|| CodecError::malformed("material fixed skip escapes entry"))?;
        slot.fixed = ctx.record_fragment(owner, extent, || {
            ctx.decode_part(
                output,
                extent,
                spec("freecad.material_list_material")?,
                |_| {
                    let mut row = extent;
                    Ok(BinaryMaterial {
                        ambient: row.req_u32_le()?,
                        diffuse: row.req_u32_le()?,
                        specular: row.req_u32_le()?,
                        emissive: row.req_u32_le()?,
                        shininess: finite32(&mut row)?,
                        transparency: finite32(&mut row)?,
                    })
                },
            )
        })?;
        if length < 24 {
            return Ok((EntryContents::Material { version, values }, sources));
        }
    }
    if version == 3 {
        'strings: for (index, slot) in values.iter_mut().enumerate() {
            for (field, target) in [&mut slot.image, &mut slot.image_path, &mut slot.uuid]
                .into_iter()
                .enumerate()
            {
                let start = cursor.position();
                let end = View::u32_le_at(cursor.unread(), 0)
                    .and_then(|length| usize::try_from(length).ok())
                    .and_then(|length| start.checked_add(4)?.checked_add(length))
                    .filter(|end| *end <= cursor.end());
                let extent = cursor
                    .child(start, end.unwrap_or(cursor.end()))
                    .ok_or_else(|| CodecError::malformed("material string extent escapes entry"))?;
                cursor
                    .skip(extent.remaining())
                    .ok_or_else(|| CodecError::malformed("material string skip escapes entry"))?;
                *target = ctx.record_fragment(owner, extent, || {
                    ctx.decode_part(
                        output,
                        extent,
                        framed_spec("freecad.material_list_string", extent, &[(0, 4)])?,
                        |_| {
                            let mut source = extent;
                            let count = usize::try_from(source.req_u32_le()?)
                                .map_err(CodecError::malformed)?;
                            let text =
                                std::str::from_utf8(source.req_take(count)?).map_err(|_| {
                                    CodecError::malformed("material string is not UTF-8")
                                })?;
                            if text.contains('\0') {
                                return Err(CodecError::malformed("material string contains NUL"));
                            }
                            ctx.retain_bytes(u64_from_index(text.len()), "entry material string")?;
                            Ok(text.to_owned())
                        },
                    )
                })?;
                if target.is_some() {
                    sources[index][field] = Some(extent);
                }
                if end.is_none() {
                    break 'strings;
                }
            }
        }
    }
    Ok((EntryContents::Material { version, values }, sources))
}
