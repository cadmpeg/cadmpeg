// SPDX-License-Identifier: Apache-2.0
//! Metadata type-table parts.

use crate::layout::meta_type_descriptor;
use crate::native::MetaTypeRecord;
use crate::rse::SegmentMeta;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

/// The admitted descriptor of each type-table slot. A slot whose descriptor
/// or whose whole table was given up is `None`.
pub(crate) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    token: &str,
    meta: &SegmentMeta<'a>,
) -> Result<Option<Vec<Option<crate::records::TypeDescriptor>>>, CodecError> {
    let stream = || format!("RSeStorage/M{token}");
    let expanded = || format!("RSeStorage/M{token}:expanded");
    ctx.decode_part(
        output,
        meta.source,
        super::part_spec("cfb.stream", meta.source, stream)?,
        |member| {
            ctx.decode_part(
                member,
                meta.compressed,
                super::part_spec("inventor.meta_body", meta.compressed, stream)?,
                |body| {
                    let frame = crate::records::frame_type_table(meta.body)?;
                    let table = ctx.decode_part(
                        body,
                        frame.extent,
                        super::part_spec("inventor.meta_forward_section", frame.extent, expanded)?,
                        |table| {
                            if !crate::records::type_count_admitted(frame.count) {
                                return Err(CodecError::malformed(
                                    "Inventor type table contains more than 256 descriptors",
                                ));
                            }
                            let mut admitted = ctx.alloc_filled(
                                frame.count,
                                None,
                                "allocate Inventor admitted source types",
                            )?;
                            let mut entries = frame.payload;
                            for (slot, admitted) in admitted.iter_mut().enumerate() {
                                let source = crate::reader::take_child(
                                    &mut entries,
                                    meta_type_descriptor::LEN,
                                    "type descriptor",
                                )?;
                                let index = u32::try_from(slot).map_err(|_| {
                                    CodecError::malformed(
                                        "Inventor metadata type index exceeds u32",
                                    )
                                })?;
                                let id = format!("inventor:rse:meta-type#{token}-{index}");
                                *admitted = ctx.decode_part(
                                    table,
                                    source,
                                    super::part_spec("inventor.type_descriptor", source, expanded)?,
                                    |record| {
                                        let value =
                                            crate::records::parse_type_descriptor(source, index)?;
                                        record.insert_native(
                                            ctx,
                                            "meta_types",
                                            MetaTypeRecord {
                                                id,
                                                token: token.to_owned(),
                                                index: value.index,
                                                type_id: super::hex(&value.id),
                                                fields: value.fields,
                                            },
                                        )?;
                                        Ok(value)
                                    },
                                )?;
                            }
                            Ok(admitted)
                        },
                    )?;
                    match table {
                        Some(admitted) => Ok(admitted),
                        None => ctx.alloc_filled(
                            frame.count,
                            None,
                            "allocate Inventor given-up source types",
                        ),
                    }
                },
            )
        },
    )
    .map(Option::flatten)
}

#[cfg(test)]
mod tests;
