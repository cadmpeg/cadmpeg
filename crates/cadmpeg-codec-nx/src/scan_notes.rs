// SPDX-License-Identifier: Apache-2.0
//! Notes shared by NX decode and inspection summaries.

use std::fmt::{self, Write as _};

use cadmpeg_core::bytes::assemble_u32_be;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::decode::{self, Scan};
use crate::parasolid::StreamKind;

// Templates contain fixed text, at most four decimal counts, and one hex word.
const MAX_SCAN_NOTE_BYTES: usize = 512;

/// Classify a scan and build its inspection and decode notes.
pub(super) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
) -> Result<(crate::dialect::LayerClassification, Vec<String>), CodecError> {
    let c = &scan.container;
    let header_entry_count = c.entry_count(crate::container::Region::Header);
    let footer_entry_count = c.entry_count(crate::container::Region::Footer);
    let mut notes = Vec::new();
    match c.layout {
        crate::container::ContainerLayout::LegacyCfb { .. } => push_note(
            ctx,
            &mut notes,
            format_args!(
                "legacy CFB container: {} directory entr{}",
                header_entry_count,
                if header_entry_count == 1 { "y" } else { "ies" },
            ),
        )?,
        crate::container::ContainerLayout::Modern {
            file_tag,
            footer_offset,
            footer_fingerprint,
            ..
        } => push_note(
            ctx,
            &mut notes,
            format_args!(
                "SPLMSSTR container: file tag {}, footer offset {}, {} HEADER and {} FOOTER directory entry/ies, fingerprint {:08x}",
                file_tag,
                footer_offset,
                header_entry_count,
                footer_entry_count,
                assemble_u32_be(footer_fingerprint),
            ),
        )?,
    }
    push_note(
        ctx,
        &mut notes,
        format_args!(
            "embedded streams: {} partition, {} deltas, {} plain (cached body), {} preview/non-Parasolid",
            scan.count(StreamKind::Partition),
            scan.count(StreamKind::Deltas),
            scan.count(StreamKind::Plain),
            scan.count(StreamKind::Preview),
        ),
    )?;
    let (control_count, classified_control_count) = decode::offset_store_control_counts(c);
    if control_count != 0 {
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "NX object model: {classified_control_count} of {control_count} bounded offset-store control block(s) have an admitted complete grammar"
            ),
        )?;
    }
    let framed_om_sections = c.om_sections();
    if !framed_om_sections.is_empty() {
        let declarations = framed_om_sections
            .iter()
            .map(|(_, section)| section.types.len())
            .sum::<usize>();
        let fields = framed_om_sections
            .iter()
            .map(|(_, section)| section.fields.len())
            .sum::<usize>();
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "NX object model: {} size-framed section(s), {} class declaration(s), {} field declaration(s)",
                framed_om_sections.len(),
                declarations,
                fields
            ),
        )?;
    }
    let om_sections = c.indexed_om_sections();
    if !om_sections.is_empty() {
        let entities = om_sections
            .iter()
            .filter_map(|(_, section)| section.as_fixed())
            .map(<[crate::om::FixedEntityRecord<'_>]>::len)
            .sum::<usize>();
        let blocks = om_sections
            .iter()
            .filter_map(|(_, section)| section.as_offset_only())
            .map(|(_control, _, records)| records.len() + 1)
            .sum::<usize>();
        if blocks == 0 {
            push_note(
                ctx,
                &mut notes,
                format_args!(
                    "NX object model: {} indexed section(s), {} bounded entity record(s)",
                    om_sections.len(),
                    entities
                ),
            )?;
        } else {
            push_note(
                ctx,
                &mut notes,
                format_args!(
                    "NX object model: {} indexed section(s), {} ID-bounded entity record(s), {} offset-only data block(s)",
                    om_sections.len(),
                    entities,
                    blocks
                ),
            )?;
        }
    }
    if !scan.has_parasolid()
        && c.entries
            .iter()
            .any(|e| e.name.contains("ExternalReferences"))
    {
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "no inline Parasolid geometry (assembly .prt: geometry in external child parts)"
            ),
        )?;
    }
    Ok((crate::dialect::classify_layers(scan), notes))
}

struct NoteBuffer {
    bytes: [u8; MAX_SCAN_NOTE_BYTES],
    len: usize,
}

impl NoteBuffer {
    fn new() -> Self {
        Self {
            bytes: [0; MAX_SCAN_NOTE_BYTES],
            len: 0,
        }
    }
}

impl fmt::Write for NoteBuffer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let dest = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        dest.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn push_note(
    ctx: &DecodeContext<'_>,
    notes: &mut Vec<String>,
    args: fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let mut measured = NoteBuffer::new();
    measured
        .write_fmt(args)
        .map_err(|_| {
            ctx.refuse_codec_limit(
                "nx scan note text",
                u64::try_from(MAX_SCAN_NOTE_BYTES).unwrap_or(u64::MAX),
                u64::try_from(MAX_SCAN_NOTE_BYTES + 1).unwrap_or(u64::MAX),
            )
        })?;
    ctx.charge_collection_items(1, "nx scan notes")?;
    ctx.charge_retained(
        u64::try_from(measured.len).unwrap_or(u64::MAX),
        "nx scan note text",
    )?;
    notes
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("nx scan notes", 0, 1))?;
    let mut note = String::new();
    note.try_reserve_exact(measured.len)
        .map_err(|_| ctx.refuse_codec_limit("nx scan note text", 0, 1))?;
    note.write_fmt(args)
        .map_err(|_| ctx.refuse_codec_limit("nx scan note text", 0, 1))?;
    notes.push(note);
    Ok(())
}
