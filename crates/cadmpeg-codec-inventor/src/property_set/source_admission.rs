// SPDX-License-Identifier: Apache-2.0
//! Source-owned property-set header records.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

use super::{PropertySetDescriptor, PropertySetStream};
use crate::native::PropertySetRecord;

/// Appendix C `inventor.property_set_header` framing: the u32 section count at
/// header +24. The section offsets in the pairs are `inventor.property_section`
/// framing.
const SECTION_COUNT_OFFSET: u64 = 24;
const SECTION_COUNT_END: u64 = SECTION_COUNT_OFFSET + 4;

/// Form a header from its actual Appendix C extent. Held syntax values can form
/// native content without claiming a record layout or source read.
pub(crate) fn stage_header<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    descriptor: &PropertySetDescriptor<'a>,
    parsed: &PropertySetStream<'a>,
) -> Result<bool, CodecError> {
    let id = format!("inventor:property:set#{}", descriptor.stream.directory_id());
    let Some(_) = descriptor.source else {
        output.insert_native(
            ctx,
            "property_sets",
            PropertySetRecord {
                id,
                path: descriptor.path.clone(),
                directory_id: descriptor.stream.directory_id(),
                version: parsed.version,
                system_identifier: parsed.system_identifier,
                clsid: super::hex(&parsed.clsid),
                section_count: u64_from_index(parsed.sections.len()),
            },
        )?;
        return Ok(true);
    };
    stage_source_header(ctx, output, descriptor, Some(parsed)).map(|(formed, _)| formed)
}

/// Admit actual source fields independently of a cached parse success/failure.
/// The optional expected value is the held-syntax source-validation boundary.
pub(crate) fn stage_source_header<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    descriptor: &PropertySetDescriptor<'a>,
    expected: Option<&PropertySetStream<'a>>,
) -> Result<(bool, usize), CodecError> {
    let source = descriptor
        .source
        .ok_or_else(|| CodecError::malformed("held property syntax has no source header"))?;
    ctx.capture_part_extent(source)?;
    let extent = super::header_extent(source)?;
    let directory = super::frame_header_directory(extent)?;
    let count = directory.directories.len();
    let id = format!("inventor:property:set#{}", descriptor.stream.directory_id());
    let level = PartLevel::parse("inventor.property_set_header").ok_or_else(|| {
        CodecError::malformed("OLE property-set header is absent from Appendix C")
    })?;
    let start = u64_from_index(extent.start());
    let framing = vec![start + SECTION_COUNT_OFFSET..start + SECTION_COUNT_END];
    let result = ctx.decode_part(output, extent, PartSpec::new(level, framing), |stage| {
        ctx.record(&id, extent, || {
            // This second parse constructs the native values from actual reads
            // inside their record scope, rather than assigning earlier reads.
            let mut header = super::parse_header(extent)?;
            header.directories.sort_by_key(|(_, offset)| *offset);
            if let Some(parsed) = expected {
                if header.version != parsed.version
                    || header.system_identifier != parsed.system_identifier
                    || header.clsid != parsed.clsid
                    || header.directories.len() != parsed.sections.len()
                    || header
                        .directories
                        .iter()
                        .zip(&parsed.sections)
                        .any(|((fmtid, _), section)| *fmtid != section.fmtid)
                {
                    return Err(CodecError::malformed(
                        "OLE staged header differs from its actual source",
                    ));
                }
            }
            stage.insert_native(
                ctx,
                "property_sets",
                PropertySetRecord {
                    id: id.clone(),
                    path: descriptor.path.clone(),
                    directory_id: descriptor.stream.directory_id(),
                    version: header.version,
                    system_identifier: header.system_identifier,
                    clsid: super::hex(&header.clsid),
                    section_count: u64_from_index(header.directories.len()),
                },
            )
        })
    })?;
    Ok((result.is_some(), count))
}

/// Locate a section from the actual header directory and size fields. The
/// directory order is normalized exactly as in the property-set parser.
pub(crate) fn section_source<'a>(
    ctx: &DecodeContext<'a>,
    descriptor: &PropertySetDescriptor<'a>,
    ordinal: usize,
) -> Result<(cadmpeg_core::decode::View<'a>, [u8; 16]), CodecError> {
    let source = descriptor
        .source
        .ok_or_else(|| CodecError::malformed("held property syntax has no source section"))?;
    ctx.capture_part_extent(source)?;
    let header_source = super::header_extent(source)?;
    let mut header = super::frame_header_directory(header_source)?;
    header.directories.sort_by_key(|(_, offset)| *offset);
    let mut previous_end = header_source.window().len();
    let mut selected = None;
    for (index, (fmtid, offset)) in header.directories.into_iter().enumerate() {
        let section = super::section_extent(source, offset, previous_end)?;
        previous_end = section.end() - source.start();
        if index == ordinal {
            selected = Some((section, fmtid));
        }
    }
    super::require_zero_range(
        source.window(),
        previous_end,
        source.window().len(),
        "stream suffix",
    )?;
    selected.ok_or_else(|| {
        CodecError::malformed("property section ordinal is outside its source directory")
    })
}

#[cfg(test)]
mod tests;
