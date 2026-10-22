// SPDX-License-Identifier: Apache-2.0
//! Parent-first local recovery with source-proven cursor advances.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};

use super::layout::{add, signature, u16_at, u32_at, u64_at};
use super::{append, child, frame, push, FrameParent, FramedPart, ZipEntry};

/// Complete central-directory framing or a bounded local-only prefix.
#[derive(Debug)]
pub enum ZipScan<'a> {
    /// Every member identity comes from an actual complete central directory.
    Complete {
        /// Original central-governed members.
        entries: Vec<ZipEntry<'a>>,
    },
    /// No complete directory was admitted; the walk stopped at an exact boundary.
    Local {
        /// Complete local members whose ends were physically established.
        entries: Vec<LocalEntry<'a>>,
        /// Why the local walk cannot establish another complete member.
        stop: RecoveryStop<'a>,
    },
}

/// A complete physical local member without an invented central declaration.
#[derive(Debug)]
pub struct LocalEntry<'a> {
    name: View<'a>,
    payload: View<'a>,
    frame: usize,
    method: u16,
    flags: u16,
}
impl<'a> LocalEntry<'a> {
    /// Actual name bytes in this local header.
    pub fn name(&self) -> View<'a> {
        self.name
    }
    /// Exactly bounded stored or compressed source extent.
    pub fn payload(&self) -> View<'a> {
        self.payload
    }
    /// Actual zip.entry frame index.
    pub fn frame_index(&self) -> usize {
        self.frame
    }
    /// Method declared by the actual local header.
    pub fn method(&self) -> u16 {
        self.method
    }
    /// Flags declared by the actual local header.
    pub fn flags(&self) -> u16 {
        self.flags
    }
}

/// A source boundary that prevents complete local-only framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryReason {
    /// The source ended after the last proved local member.
    SourceEnd,
    /// An actual following directory/end/other structure needs its own locator.
    FollowingStructure,
    /// The reached local header is incomplete.
    HeaderIncomplete,
    /// The header's extra fields cannot supply unambiguous required sizes.
    InvalidExtraFields,
    /// Flag 3 leaves the compressed size unavailable.
    SizeUnavailable,
    /// The declared payload extends beyond the available source.
    PayloadIncomplete,
    /// The descriptor is incomplete, ambiguous or lacks required integrity facts.
    DescriptorUnavailable,
}

/// An ephemeral source stop, not a completed member or a semantic record.
#[derive(Debug)]
pub struct RecoveryStop<'a> {
    reason: RecoveryReason,
    available: View<'a>,
    declared_end: Option<u64>,
    frame: Option<usize>,
}
impl<'a> RecoveryStop<'a> {
    /// Exact condition observed at the reached boundary.
    pub fn reason(&self) -> RecoveryReason {
        self.reason
    }
    /// Original bytes available at the stopped unit or boundary.
    pub fn available(&self) -> View<'a> {
        self.available
    }
    /// Actual declared end when its source fields and address arithmetic admit one.
    pub fn declared_end(&self) -> Option<u64> {
        self.declared_end
    }
    /// Actual incomplete header/entry frame, when its available prefix is proved.
    pub fn frame_index(&self) -> Option<usize> {
        self.frame
    }
}

/// Frame complete archives, or recover a parent-first local-header prefix.
///
/// Missing directories supply no member count, central frame or end record.
/// The walk starts only at the parent's first local signature, and each later
/// header must begin at the proved end of the previous member/descriptor. It
/// never searches payload bytes for signatures. Unknown flag-3 stored sizes
/// stop the walk; unknown compressed sizes need a consumed-length decoder.
///
/// # Errors
/// Returns invalid provenance, a missing parent-first local signature, a
/// complete-directory framing error, or a caller-budget/allocation refusal.
pub fn scan<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<ZipScan<'a>, String> {
    ctx.capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    if super::layout::directory(source.window()).is_ok() {
        return frame(ctx, source, out).map(|entries| ZipScan::Complete { entries });
    }
    if !signature(source.window(), 0, b"PK\x03\x04") {
        return Err("ZIP local recovery requires a local header at the parent's first byte".into());
    }
    let archive = push(
        ctx,
        source,
        out,
        "zip.archive",
        FrameParent::File,
        0..source.window().len(),
        (vec![], vec![]),
    )?;
    let mut entries = Vec::new();
    let mut at = 0;
    loop {
        if at == source.window().len() {
            return finish(
                ctx,
                source,
                out,
                entries,
                (RecoveryReason::SourceEnd, at, None, None),
            );
        }
        if !signature(source.window(), at, b"PK\x03\x04") {
            return finish(
                ctx,
                source,
                out,
                entries,
                (RecoveryReason::FollowingStructure, at, None, None),
            );
        }
        match member(ctx, source, out, archive, at)? {
            Attempt::Member(entry, next) => {
                append(ctx, &mut entries, entry)?;
                at = next;
            }
            Attempt::Stop(reason, start, declared_end, frame) => {
                return finish(
                    ctx,
                    source,
                    out,
                    entries,
                    (reason, start, declared_end, frame),
                )
            }
        }
    }
}

enum Attempt<'a> {
    Member(LocalEntry<'a>, usize),
    Stop(RecoveryReason, usize, Option<u64>, Option<usize>),
}

fn finish<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &[FramedPart<'a>],
    entries: Vec<LocalEntry<'a>>,
    (reason, start, declared_end, frame): (RecoveryReason, usize, Option<u64>, Option<usize>),
) -> Result<ZipScan<'a>, String> {
    super::PartTable::load()?.validate(ctx, out)?;
    Ok(ZipScan::Local {
        entries,
        stop: RecoveryStop {
            reason,
            available: child(source, start..source.window().len())?,
            declared_end,
            frame,
        },
    })
}

fn member<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    archive: usize,
    at: usize,
) -> Result<Attempt<'a>, String> {
    let bytes = source.window();
    let fixed = add(at, 30)?;
    if fixed > bytes.len() {
        let frame = header(ctx, source, out, archive, at, bytes.len(), None)?;
        return Ok(Attempt::Stop(
            RecoveryReason::HeaderIncomplete,
            at,
            None,
            Some(frame),
        ));
    }
    let name = fixed;
    let extra = add(name, usize::from(u16_at(bytes, add(at, 26)?)?))?;
    let payload = add(extra, usize::from(u16_at(bytes, add(at, 28)?)?))?;
    if payload > bytes.len() {
        let frame = header(
            ctx,
            source,
            out,
            archive,
            at,
            bytes.len(),
            Some(name..extra),
        )?;
        let declared = u64_from_index(source.start()).checked_add(u64_from_index(payload));
        return Ok(Attempt::Stop(
            RecoveryReason::HeaderIncomplete,
            at,
            declared,
            Some(frame),
        ));
    }
    let header = header(ctx, source, out, archive, at, payload, Some(name..extra))?;
    let actual64 = match super::zip64::field(bytes, extra..payload) {
        Ok(value) => value,
        Err(_) => {
            return Ok(Attempt::Stop(
                RecoveryReason::InvalidExtraFields,
                extra,
                None,
                None,
            ))
        }
    };
    if actual64
        .as_ref()
        .is_some_and(|field| field.end - field.start < 16)
    {
        return Ok(Attempt::Stop(
            RecoveryReason::InvalidExtraFields,
            extra,
            None,
            None,
        ));
    }
    let local64 = super::members::extras(
        ctx,
        source,
        out,
        "zip.local_extra_field",
        header,
        extra..payload,
        0b11,
    )?;
    let compressed32 = u32_at(bytes, add(at, 18)?)?;
    let uncompressed32 = u32_at(bytes, add(at, 22)?)?;
    let (compressed, uncompressed) = if local64 {
        let actual = super::zip64::field(bytes, extra..payload)?
            .ok_or("missing just-framed ZIP64 local field")?;
        (
            u64_at(bytes, add(actual.start, 8)?)?,
            u64_at(bytes, actual.start)?,
        )
    } else if compressed32 == u32::MAX || uncompressed32 == u32::MAX {
        return Ok(Attempt::Stop(
            RecoveryReason::InvalidExtraFields,
            extra,
            None,
            None,
        ));
    } else {
        (u64::from(compressed32), u64::from(uncompressed32))
    };
    let flags = u16_at(bytes, add(at, 6)?)?;
    let method = u16_at(bytes, add(at, 8)?)?;
    if flags & 8 != 0 && compressed == 0 {
        return Ok(Attempt::Stop(
            RecoveryReason::SizeUnavailable,
            payload,
            None,
            None,
        ));
    }
    let available = bytes.len() - payload;
    let declared_end = u64_from_index(source.start())
        .checked_add(u64_from_index(payload))
        .and_then(|start| start.checked_add(compressed));
    if compressed > u64_from_index(available) {
        let entry = push(
            ctx,
            source,
            out,
            "zip.entry",
            FrameParent::Part(archive),
            payload..bytes.len(),
            (vec![], vec![]),
        )?;
        return Ok(Attempt::Stop(
            RecoveryReason::PayloadIncomplete,
            payload,
            declared_end,
            Some(entry),
        ));
    }
    let end = add(
        payload,
        usize::try_from(compressed).map_err(|_| "ZIP local payload size exceeds usize")?,
    )?;
    let entry = push(
        ctx,
        source,
        out,
        "zip.entry",
        FrameParent::Part(archive),
        payload..end,
        (vec![], vec![]),
    )?;
    let next = if flags & 8 != 0 {
        if method != 0 || flags & 1 != 0 || compressed != uncompressed {
            return Ok(Attempt::Stop(
                RecoveryReason::DescriptorUnavailable,
                end,
                None,
                None,
            ));
        }
        if signature(bytes, end, b"PK\x07\x08")
            && add(end, if local64 { 24 } else { 16 })? > bytes.len()
        {
            return Ok(Attempt::Stop(
                RecoveryReason::DescriptorUnavailable,
                end,
                None,
                None,
            ));
        }
        let crc = crate::zip_fixture::crc32(&bytes[payload..end]);
        let values = (crc, compressed, uncompressed, local64);
        if super::members::descriptor_layout(bytes, end, bytes.len(), values).is_err() {
            return Ok(Attempt::Stop(
                RecoveryReason::DescriptorUnavailable,
                end,
                None,
                None,
            ));
        }
        let crc_at =
            super::members::descriptor(ctx, source, out, archive, end, bytes.len(), values)?;
        add(crc_at, if local64 { 20 } else { 12 })?
    } else {
        end
    };
    Ok(Attempt::Member(
        LocalEntry {
            name: child(source, name..extra)?,
            payload: child(source, payload..end)?,
            frame: entry,
            method,
            flags,
        },
        next,
    ))
}

fn header<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    archive: usize,
    start: usize,
    end: usize,
    name: Option<std::ops::Range<usize>>,
) -> Result<usize, String> {
    let mut framing = Vec::new();
    for relative in [0..4, 6..8, 18..22, 26..30] {
        let from = add(start, relative.start)?;
        let to = add(start, relative.end)?.min(end);
        if from < to {
            append(ctx, &mut framing, from..to)?;
        }
    }
    let mut dispatch = Vec::new();
    let method = add(start, 8)?..add(start, 10)?.min(end);
    if method.start < method.end {
        append(ctx, &mut dispatch, method)?;
    }
    if let Some(name) = name {
        let to = name.end.min(end);
        if name.start < to {
            append(ctx, &mut dispatch, name.start..to)?;
        }
    }
    push(
        ctx,
        source,
        out,
        "zip.local_header",
        FrameParent::Part(archive),
        start..end,
        (framing, dispatch),
    )
}
