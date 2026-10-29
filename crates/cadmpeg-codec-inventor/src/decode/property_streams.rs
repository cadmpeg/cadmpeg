// SPDX-License-Identifier: Apache-2.0
//! OLE property-set stream parts.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

use crate::property_set::{PropertySection, PropertySetDescriptor};

pub(super) struct PropertyStreamProjection<'a> {
    pub(super) header_formed: bool,
    pub(super) sections: Vec<(usize, PropertySection<'a>)>,
}

/// Only this committed result can contribute global product metadata. Failure
/// to locate a child gives up its actual CFB stream, never the compound root.
pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    descriptor: &PropertySetDescriptor<'a>,
    preview_ordinal: &mut usize,
) -> Result<Option<PropertyStreamProjection<'a>>, CodecError> {
    let source = descriptor
        .source
        .ok_or_else(|| CodecError::malformed("held property syntax has no source stream"))?;
    ctx.decode_part(
        output,
        source,
        super::part_spec("cfb.stream", source, || descriptor.path.clone())?,
        |stream_stage| {
            let (header_formed, count) =
                crate::property_set::source_admission::stage_source_header(
                    ctx,
                    stream_stage,
                    descriptor,
                    None,
                )?;
            let mut sections = Vec::new();
            for ordinal in 0..count {
                if let Some(section) = super::property_sections::stage(
                    ctx,
                    stream_stage,
                    descriptor,
                    ordinal,
                    preview_ordinal,
                )? {
                    sections.push((ordinal, section));
                }
            }
            Ok(PropertyStreamProjection {
                header_formed,
                sections,
            })
        },
    )
}

#[cfg(test)]
mod tests;
