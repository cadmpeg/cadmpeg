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
    let header_entry_count = c.entry_count(ctx, crate::container::Region::Header)?;
    let footer_entry_count = c.entry_count(ctx, crate::container::Region::Footer)?;
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
            scan.count(ctx, StreamKind::Partition)?,
            scan.count(ctx, StreamKind::Deltas)?,
            scan.count(ctx, StreamKind::Plain)?,
            scan.count(ctx, StreamKind::Preview)?,
        ),
    )?;
    let (control_count, classified_control_count) = decode::offset_store_control_counts(ctx, c)?;
    if control_count != 0 {
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "NX object model: {classified_control_count} of {control_count} bounded offset-store control block(s) have an admitted complete grammar"
            ),
        )?;
    }
    let mut framed_storage = ctx.reserve_scoped(0, "NX scan-note framed sections")?;
    let framed_om_sections = framed_storage.with_storage(|| c.om_sections(ctx))?;
    if !framed_om_sections.is_empty() {
        let (mut declarations, mut fields) = (0usize, 0usize);
        for (_, section) in
            ctx.admit_iter(&framed_om_sections, "count NX OM declarations and fields")?
        {
            declarations = declarations
                .checked_add(section.types.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("count NX OM declarations", u64::MAX, u64::MAX)
                })?;
            fields = fields
                .checked_add(section.fields.len())
                .ok_or_else(|| ctx.refuse_codec_limit("count NX OM fields", u64::MAX, u64::MAX))?;
        }
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
    drop(framed_om_sections);
    drop(framed_storage);
    let mut indexed_storage = ctx.reserve_scoped(0, "NX scan-note indexed sections")?;
    let om_sections = indexed_storage.with_storage(|| c.indexed_om_sections(ctx))?;
    if !om_sections.is_empty() {
        let (mut entities, mut blocks) = (0usize, 0usize);
        for (_, section) in
            ctx.admit_iter(&om_sections, "count NX indexed entities and offset blocks")?
        {
            if let Some(records) = section.as_fixed() {
                entities = entities.checked_add(records.len()).ok_or_else(|| {
                    ctx.refuse_codec_limit("count NX indexed entities", u64::MAX, u64::MAX)
                })?;
            }
            if let Some((_control, _, records)) = section.as_offset_only() {
                blocks = records
                    .len()
                    .checked_add(1)
                    .and_then(|count| blocks.checked_add(count))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("count NX offset blocks", u64::MAX, u64::MAX)
                    })?;
            }
        }
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
    drop(om_sections);
    drop(indexed_storage);
    if !scan.has_parasolid(ctx)? && c.has_external_references(ctx)? {
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "no inline Parasolid geometry (assembly .prt: geometry in external child parts)"
            ),
        )?;
    }
    Ok((crate::dialect::classify_layers(ctx, scan)?, notes))
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
    measured.write_fmt(args).map_err(|_| {
        ctx.refuse_codec_limit(
            "nx scan note text",
            cadmpeg_core::decode::u64_from_index(MAX_SCAN_NOTE_BYTES),
            cadmpeg_core::decode::u64_from_index(MAX_SCAN_NOTE_BYTES + 1),
        )
    })?;
    let text = std::str::from_utf8(&measured.bytes[..measured.len])
        .map_err(|_| CodecError::malformed("NX scan note contains invalid UTF-8"))?;
    let note = ctx.copy_retained_text(text, "nx scan note text")?;
    ctx.push_vec(notes, note, "nx scan notes")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::push_note;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    #[test]
    fn scan_note_retention_refuses_at_the_text_copy() {
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::RetainedBytes,
            "nx scan note text",
            |ctx| {
                let mut notes = Vec::new();
                push_note(ctx, &mut notes, format_args!("count {}", 42))?;
                Ok(notes)
            },
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "nx scan note text")
        );
    }

    #[test]
    fn scan_note_keeps_the_measured_utf8_text() {
        let notes = crate::test_support::with_decode_context(|ctx| {
            let mut notes = Vec::new();
            push_note(ctx, &mut notes, format_args!("count {} μ", 42))?;
            Ok::<_, CodecError>(notes)
        })
        .unwrap();
        assert_eq!(notes, ["count 42 μ"]);
    }
}
