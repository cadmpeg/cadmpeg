// SPDX-License-Identifier: Apache-2.0
//! Independent ZIP physical framing and stored-entry CRC resealing.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

use super::{FrameParent, FramedPart, PartTable};
use layout::add;

mod layout;
mod members;
mod rebuild;
mod rebuild_member;
mod recovery;
mod zip64;

pub use rebuild::{rebuild, Edit};
pub use rebuild_member::rebuild_member;
pub use recovery::{scan, LocalEntry, RecoveryReason, RecoveryStop, ZipScan};

/// Actual source views and physical records for one central-directory member.
#[derive(Debug)]
pub struct ZipEntry<'a> {
    name: View<'a>,
    payload: View<'a>,
    frame: usize,
    method: u16,
    flags: u16,
    local_crc: usize,
    central_crc: usize,
    descriptor_crc: Option<usize>,
    uncompressed: u64,
    local_flags: u16,
}
impl<'a> ZipEntry<'a> {
    /// Original name bytes in the governing central header.
    pub fn name(&self) -> View<'a> {
        self.name
    }
    /// Original compressed extent; method zero is directly stored content.
    pub fn payload(&self) -> View<'a> {
        self.payload
    }
    /// Index of this member's actual zip.entry frame in the caller's output.
    pub fn frame_index(&self) -> usize {
        self.frame
    }
    /// Original method number; no decompression or method substitution occurs.
    pub fn method(&self) -> u16 {
        self.method
    }
    /// Governing central flags, including encryption and descriptor indicators.
    pub fn flags(&self) -> u16 {
        self.flags
    }
}

/// Frame a complete single-disk ZIP archive from the actual source view.
///
/// Digital-signature bytes are preserved without cryptographic verification.
/// Compressed entries retain their physical spans; their expanded children need
/// genuine decompression provenance. No source bytes count as semantic reads.
/// The returned member views can host the STEP and FreeCAD member framers.
///
/// # Errors
/// Returns ambiguous/incomplete boundaries, multi-disk requirements,
/// overlapping records, invalid provenance or resource/allocation refusal.
pub fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<Vec<ZipEntry<'a>>, String> {
    ctx.capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    let directory = layout::directory(source.window())?;
    let archive = push(
        ctx,
        source,
        out,
        "zip.archive",
        FrameParent::File,
        0..source.window().len(),
        (vec![], vec![]),
    )?;
    push(
        ctx,
        source,
        out,
        "zip.end_record",
        FrameParent::Part(archive),
        directory.end_record..source.window().len(),
        (
            vec![directory.end_record..add(directory.end_record, 22)?],
            vec![],
        ),
    )?;
    if let Some(end) = directory.zip64 {
        push(
            ctx,
            source,
            out,
            "zip.zip64_end_locator",
            FrameParent::Part(archive),
            end.locator..directory.end_record,
            (vec![end.locator..directory.end_record], vec![]),
        )?;
        push(
            ctx,
            source,
            out,
            "zip.zip64_end_record",
            FrameParent::Part(archive),
            end.start..end.locator,
            (vec![end.start..add(end.start, 56)?], vec![]),
        )?;
    }
    let central = push(
        ctx,
        source,
        out,
        "zip.central_directory",
        FrameParent::Part(archive),
        directory.start..directory.central_end,
        (vec![], vec![]),
    )?;
    let mut entries = Vec::new();
    let mut at = directory.start;
    for _ in 0..directory.count {
        let (entry, end) = members::member(ctx, source, out, directory, at, (archive, central))?;
        append(ctx, &mut entries, entry)?;
        at = end;
    }
    if let Some(signature) = directory.signature {
        let parent = if signature.inside { central } else { archive };
        let index = push(
            ctx,
            source,
            out,
            "zip.digital_signature",
            FrameParent::Part(parent),
            signature.start..signature.end,
            (vec![signature.start..add(signature.start, 6)?], vec![]),
        )?;
        if signature.inside {
            let proof = DirectorySignature {
                directory: child(source, directory.start..directory.central_end)?,
                signature: child(source, signature.start..signature.end)?,
            };
            out.get_mut(index)
                .ok_or("missing just-framed ZIP signature")?
                .bind_zip_directory_parent(ctx, &proof, archive)?;
        }
    }
    PartTable::load()?.validate(ctx, out)?;
    Ok(entries)
}

pub(super) struct DirectorySignature<'a> {
    directory: View<'a>,
    signature: View<'a>,
}
impl<'a> DirectorySignature<'a> {
    pub(super) fn directory(&self) -> View<'a> {
        self.directory
    }
    pub(super) fn signature(&self) -> View<'a> {
        self.signature
    }
}

fn child(source: View<'_>, extent: Range<usize>) -> Result<View<'_>, String> {
    let start = add(source.start(), extent.start)?;
    let end = add(source.start(), extent.end)?;
    source
        .child(start, end)
        .ok_or_else(|| "ZIP part exceeds its actual source extent".into())
}

fn push<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    level: &str,
    parent: FrameParent,
    extent: Range<usize>,
    (framing, dispatch): (Vec<Range<usize>>, Vec<Range<usize>>),
) -> Result<usize, String> {
    let view = child(source, extent)?;
    let convert = |ranges: Vec<Range<usize>>| {
        let mut result = Vec::new();
        for range in ranges {
            append(
                ctx,
                &mut result,
                u64_from_index(add(source.start(), range.start)?)
                    ..u64_from_index(add(source.start(), range.end)?),
            )?;
        }
        Ok::<_, String>(result)
    };
    let part = FramedPart::new(
        ctx,
        PartLevel::parse(level).ok_or("missing ZIP Appendix C level")?,
        view,
        parent,
        convert(framing)?,
        convert(dispatch)?,
    )?;
    let index = out.len();
    append(ctx, out, part)?;
    Ok(index)
}

fn append<T>(ctx: &DecodeContext<'_>, out: &mut Vec<T>, value: T) -> Result<(), String> {
    ctx.retain_bytes(u64_from_index(size_of::<T>()), "ZIP source frame storage")
        .map_err(|error| error.to_string())?;
    out.try_reserve(1).map_err(|error| error.to_string())?;
    out.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
