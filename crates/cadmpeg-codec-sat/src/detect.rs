// SPDX-License-Identifier: Apache-2.0
//! Stream-kind detection and container inspection for bare ASM streams.

use cadmpeg_asm::acis_header;
use cadmpeg_asm::asm_header;
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::sat;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::ContainerSummary;
use std::collections::BTreeMap;

use crate::dialect::{terminator_line, StreamEvidence, TextEvidence};

/// The stream encoding a byte prefix selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamKind {
    /// `ASM BinaryFile4`/`ASM BinaryFile8` SAB.
    AsmBinary,
    /// `ACIS BinaryFile` 32-bit SAB.
    AcisBinary,
    /// Text header lines.
    Text,
    /// Not an ASM stream.
    Unknown,
}

pub(crate) fn classify(prefix: &[u8]) -> StreamKind {
    if asm_header::has_asm_magic(prefix) {
        return StreamKind::AsmBinary;
    }
    if acis_header::has_acis_magic(prefix) {
        return StreamKind::AcisBinary;
    }
    if looks_like_text_stream(prefix) {
        return StreamKind::Text;
    }
    StreamKind::Unknown
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
    let fields: Vec<&[u8]> = prefix[..line_end]
        .split(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        .filter(|field| !field.is_empty())
        .collect();
    fields.len() == 4
        && fields
            .iter()
            .all(|field| std::str::from_utf8(field).is_ok_and(|field| field.parse::<i64>().is_ok()))
        && prefix.get(line_end + 1).is_some_and(u8::is_ascii_digit)
}

pub(crate) fn confidence(prefix: &[u8]) -> Confidence {
    match classify(prefix) {
        StreamKind::AsmBinary | StreamKind::AcisBinary => Confidence::High,
        // The text opening is a weak signature shared with other numeric
        // text files, so detection defers to stronger magics.
        StreamKind::Text => Confidence::Medium,
        StreamKind::Unknown => Confidence::No,
    }
}

pub(crate) fn header_attributes(
    header: &KernelHeader,
    family: &str,
    attributes: &mut BTreeMap<String, String>,
) {
    if let Some(version) = header.save_format_version {
        attributes.insert("acis_save_format_version".to_string(), version.to_string());
    }
    if let Some(count) = header.entity_count {
        attributes.insert("kernel_entity_count".to_string(), count.to_string());
    }
    if let Some(flags) = header.flags {
        attributes.insert("kernel_flags".to_string(), flags.to_string());
    }
    if let Some(family) = &header.product_family {
        attributes.insert("product_family".to_string(), family.clone());
    }
    if let Some(version) = &header.product_version {
        attributes.insert("product_version".to_string(), version.clone());
    }
    if let Some(date) = &header.save_date {
        attributes.insert("save_date".to_string(), date.clone());
    }
    attributes.insert("kernel_family".to_string(), family.to_string());
}

pub(crate) fn inspect(
    _ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let bytes = root.window();
    let mut attributes = BTreeMap::new();
    let mut notes = Vec::new();
    let kind = classify(bytes);
    // Inspect classifies from the same evidence decode would read, so the two
    // report the same `sat:` row and the same admission for the same bytes.
    let (matched, kernel) = match kind {
        StreamKind::AsmBinary => {
            let header = asm_header::parse(bytes)
                .expect("StreamKind::AsmBinary guarantees the ASM header magic");
            let framed = asm_header::record_stream_start_with_header(bytes, &header).is_some();
            header_attributes(&header, "asm", &mut attributes);
            if header.has_history_partition() {
                notes.push(
                    "the stream declares a construction-history partition; decode reads \
                         the solved partition"
                        .to_string(),
                );
            }
            let evidence = if framed {
                StreamEvidence::AsmBinary(&header)
            } else {
                StreamEvidence::UnframedAsmBinary(&header)
            };
            crate::dialect::layers(&evidence)
        }
        StreamKind::AcisBinary => {
            let header = acis_header::parse(bytes)
                .expect("StreamKind::AcisBinary guarantees the ACIS header magic");
            let framed = acis_header::record_stream_start_with_header(bytes, &header).is_some();
            let evidence = if framed {
                StreamEvidence::AcisBinary(&header)
            } else {
                StreamEvidence::UnframedAcisBinary(&header)
            };
            header_attributes(&header, "acis", &mut attributes);
            if header.has_history_partition() {
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
            let parsed = sat::parse(bytes).map(|stream| (stream.header.as_kernel_header(), stream));
            let text = match &parsed {
                Ok((kernel, stream)) => {
                    let family = match stream.terminator {
                        sat::Terminator::Asm => "asm",
                        sat::Terminator::Acis => "acis",
                    };
                    header_attributes(kernel, family, &mut attributes);
                    attributes.insert("scale".to_string(), format!("{}", stream.header.scale));
                    attributes.insert("records".to_string(), stream.records.len().to_string());
                    attributes.insert(
                        "terminator".to_string(),
                        terminator_line(stream.terminator).to_string(),
                    );
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
        StreamKind::Unknown => {
            return Err(CodecError::WrongFormat(
                "not an ASM stream: no binary magic and no text header lines".to_string(),
            ))
        }
    };
    let losses = crate::dialect::dialect_loss(&kernel).into_iter().collect();
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched).with(kernel),
        cadmpeg_ir::ContainerKind::Stream,
        vec![ContainerEntry {
            name: "stream".to_string(),
            role: match kind {
                StreamKind::AsmBinary => "brep",
                StreamKind::AcisBinary => "acis-binary",
                StreamKind::Text => "brep-text",
                StreamKind::Unknown => unreachable!("unknown kind returned above"),
            }
            .to_string(),
            compression: "stored".to_string(),
            compressed_size: bytes.len() as u64,
            uncompressed_size: bytes.len() as u64,
            attributes,
        }],
        losses,
        notes,
    ))
}

#[cfg(test)]
mod tests;
