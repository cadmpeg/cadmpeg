// SPDX-License-Identifier: Apache-2.0
//! Stream-kind detection and container inspection for bare ASM streams.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use cadmpeg_asm::acis_header;
use cadmpeg_asm::asm_header;
use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader};
use cadmpeg_asm::sat;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::ContainerSummary;
use std::collections::BTreeMap;

use crate::dialect::{terminator_line, Family, StreamEvidence, TextEvidence};

/// The stream encoding a byte prefix selects.
#[derive(Debug)]
pub(crate) enum StreamKind {
    /// `ASM BinaryFile4`/`ASM BinaryFile8` SAB.
    AsmBinary(BinaryHeader),
    /// `ACIS BinaryFile` 32-bit SAB.
    AcisBinary(BinaryHeader),
    /// Text header lines.
    Text,
}

pub(crate) fn classify(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<Option<StreamKind>, CodecError> {
    if let Some(header) = asm_header::parse(ctx, prefix)? {
        return Ok(Some(StreamKind::AsmBinary(header)));
    }
    if let Some(header) = acis_header::parse(ctx, prefix)? {
        return Ok(Some(StreamKind::AcisBinary(header)));
    }
    if looks_like_text_stream(prefix) {
        return Ok(Some(StreamKind::Text));
    }
    Ok(None)
}

/// Whether the prefix opens like a text stream: a first line of four ASCII
/// integer fields (the four header words) followed by a counted-string line.
fn looks_like_text_stream(prefix: &[u8]) -> bool {
    if !sat::has_text_magic(prefix) {
        return false;
    }
    let Some(line_end) = prefix.iter().position(|byte| *byte == b'\n') else {
        return false;
    };
    let mut fields = prefix[..line_end]
        .split(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        .filter(|field| !field.is_empty());
    for _ in 0..4 {
        let Some(field) = fields.next() else { return false; };
        if !std::str::from_utf8(field).is_ok_and(|field| field.parse::<i64>().is_ok()) {
            return false;
        }
    }
    fields.next().is_none() && prefix.get(line_end + 1).is_some_and(u8::is_ascii_digit)
}

pub(crate) fn confidence(prefix: &[u8]) -> Confidence {
    if asm_header::has_asm_magic(prefix) || acis_header::has_acis_magic(prefix) {
        Confidence::High
    } else if looks_like_text_stream(prefix) {
        Confidence::Medium
    } else {
        Confidence::No
    }
}

pub(crate) fn header_attributes(
    ctx: &DecodeContext<'_>,
    header: &KernelHeader,
    family: Family,
    attributes: &mut BTreeMap<String, String>,
) -> Result<(), CodecError> {
    for (key, value) in [
        (
            "acis_save_format_version",
            header.save_format_version.map(u64::from),
        ),
        ("kernel_entity_count", header.entity_count),
        ("kernel_flags", header.flags),
    ] {
        if let Some(value) = value {
            let key =
                ctx.format_retained(format_args!("{key}"), "retain SAT header attribute key")?;
            let value =
                ctx.format_retained(format_args!("{value}"), "retain SAT header attribute value")?;
            ctx.insert_btree_map(attributes, key, value, "collect SAT header attributes")?;
        }
    }
    for (key, value) in [
        ("product_family", header.product_family.as_deref()),
        ("product_version", header.product_version.as_deref()),
        ("save_date", header.save_date.as_deref()),
        ("kernel_family", Some(family.as_str())),
    ] {
        if let Some(value) = value {
            let key =
                ctx.format_retained(format_args!("{key}"), "retain SAT header attribute key")?;
            let value =
                ctx.format_retained(format_args!("{value}"), "retain SAT header attribute value")?;
            ctx.insert_btree_map(attributes, key, value, "collect SAT header attributes")?;
        }
    }
    Ok(())
}

pub(crate) fn inspect(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let bytes = root.window();
    let mut attributes = BTreeMap::new();
    let mut notes = Vec::new();
    let Some(kind) = classify(ctx, bytes)? else {
        return Err(CodecError::WrongFormat(
            "not an ASM stream: no binary magic and no text header lines".to_string(),
        ));
    };
    // Inspect classifies from the same evidence decode would read, so the two
    // report the same `sat:` row and the same admission for the same bytes.
    let (matched, kernel) = match &kind {
        StreamKind::AsmBinary(header) => {
            let stream = crate::dialect::record_stream_start(bytes, Family::Asm, header);
            header_attributes(ctx, &header.metadata, Family::Asm, &mut attributes)?;
            if header.metadata.has_history_partition() {
                notes.push(
                    "the stream declares a construction-history partition; decode reads \
                         the solved partition"
                        .to_string(),
                );
            }
            let evidence = StreamEvidence::Binary {
                family: Family::Asm,
                header,
                stream,
            };
            crate::dialect::layers(&evidence)
        }
        StreamKind::AcisBinary(header) => {
            let stream = crate::dialect::record_stream_start(bytes, Family::Acis, header);
            let evidence = StreamEvidence::Binary {
                family: Family::Acis,
                header,
                stream,
            };
            header_attributes(ctx, &header.metadata, Family::Acis, &mut attributes)?;
            if header.metadata.has_history_partition() {
                notes.push(
                    "the stream declares a construction-history partition; decode reads \
                         the solved partition"
                        .into(),
                );
            }
            crate::dialect::layers(&evidence)
        }
        StreamKind::Text => {
            // The kernel header is bound here so the evidence can borrow it
            // past the arm that built it.
            let parsed = match sat::parse(ctx, bytes) {
                Ok(stream) => Ok((stream.header.as_kernel_header(ctx)?, stream)),
                Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                    return Err(error);
                }
                Err(error) => Err(error),
            };
            let text = match &parsed {
                Ok((kernel, stream)) => {
                    header_attributes(ctx, kernel, stream.terminator.into(), &mut attributes)?;
                    for (key, value) in [
                        (
                            "scale",
                            ctx.format_retained(
                                format_args!("{}", stream.header.scale().get()),
                                "retain SAT scale attribute",
                            )?,
                        ),
                        (
                            "records",
                            ctx.format_retained(
                                format_args!("{}", stream.records.len()),
                                "retain SAT record count attribute",
                            )?,
                        ),
                        (
                            "terminator",
                            ctx.format_retained(
                                format_args!("{}", terminator_line(stream.terminator)),
                                "retain SAT terminator attribute",
                            )?,
                        ),
                    ] {
                        let key = ctx.format_retained(
                            format_args!("{key}"),
                            "retain SAT inspect attribute key",
                        )?;
                        ctx.insert_btree_map(
                            &mut attributes,
                            key,
                            value,
                            "collect SAT inspect attributes",
                        )?;
                    }
                    Some(TextEvidence {
                        branch: stream.terminator,
                        header: kernel,
                    })
                }
                Err(error) => {
                    notes.push(format!("text stream does not parse: {error}"));
                    None
                }
            };
            let evidence = StreamEvidence::Text(text);
            crate::dialect::layers(&evidence)
        }
    };
    let losses = crate::dialect::dialect_loss(&kernel).into_iter().collect();
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched)
            .with_for_decode(ctx, kernel, "collect SAT dialect layers")
            .map_err(|rejected| match rejected {
                cadmpeg_core::dialect::DialectLayerError::Duplicate(layer) => {
                    CodecError::malformed(format_args!("SAT repeated dialect layer key: {layer:?}"))
                }
                cadmpeg_core::dialect::DialectLayerError::ResourceLimit(limit) => limit.into(),
            })?,
        cadmpeg_ir::ContainerKind::Stream,
        vec![ContainerEntry {
            name: "stream".to_string(),
            role: match kind {
                StreamKind::AsmBinary(_) => ContainerRole::Brep,
                StreamKind::AcisBinary(_) => ContainerRole::AcisBinary,
                StreamKind::Text => ContainerRole::BrepText,
            },
            storage: EntryStorage::verbatim(
                VerbatimLabel::Stored,
                cadmpeg_core::decode::u64_from_index(bytes.len()),
            ),
            attributes,
        }],
        losses,
        notes,
    ))
}

#[cfg(test)]
mod tests;
