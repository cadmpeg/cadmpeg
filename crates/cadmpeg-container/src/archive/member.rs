// SPDX-License-Identifier: Apache-2.0
//! Source-backed local-header and compressed-payload boundaries.

use cadmpeg_core::decode::{u64_from_index, ByteRange, DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{entry, ArchiveSnapshot, EntryRecord};

const STORAGE: &str = "ZIP member frame storage";

/// A local header and its declared payload, bounded by the next member or directory.
#[derive(Clone, Copy, Debug)]
pub struct MemberFrame<'a> {
    archive: View<'a>,
    index: usize,
    header: View<'a>,
    payload: View<'a>,
}

impl<'a> MemberFrame<'a> {
    /// Returns the original local-header extent, including its name and extras.
    pub const fn header(self) -> View<'a> {
        self.header
    }

    /// Returns only the Appendix C framing fields in the local header.
    ///
    /// # Errors
    /// Rejects fields outside the proven header extent.
    pub fn header_framing(self) -> Result<[View<'a>; 4], CodecError> {
        let field = |offset: usize, length: usize| {
            self.header
                .start()
                .checked_add(offset)
                .and_then(|start| start.checked_add(length).map(|end| (start, end)))
                .and_then(|(start, end)| self.header.child(start, end))
                .ok_or_else(|| CodecError::malformed("ZIP framing field escapes local header"))
        };
        Ok([field(0, 4)?, field(6, 2)?, field(18, 4)?, field(26, 4)?])
    }

    /// Returns the original declared compressed-payload extent.
    pub const fn payload(self) -> View<'a> {
        self.payload
    }
}

/// A payload admitted by reading its actual local header.
pub struct MemberPayload<'a>(MemberFrame<'a>);

impl<'a> MemberPayload<'a> {
    /// Returns the admitted compressed-payload extent.
    pub const fn view(&self) -> View<'a> {
        self.0.payload
    }
}

impl<'a> ArchiveSnapshot<'a> {
    /// Locates member boundaries without reading semantic values or admitting headers.
    ///
    /// The result follows directory order. An absent frame means the declared
    /// locator and actual local length fields cannot prove disjoint extents.
    /// A bad signature can still have a bounded header, whose parser then fails.
    ///
    /// # Errors
    /// Rejects foreign source views and storage or allocation limit failures.
    pub fn member_frames(
        &self,
        ctx: &DecodeContext<'a>,
    ) -> Result<Vec<Option<MemberFrame<'a>>>, CodecError> {
        ctx.capture_part_extent(self.root)?;
        let count = self.entries.len();
        let bytes = count
            .checked_mul(size_of::<usize>() + size_of::<Option<MemberFrame<'_>>>())
            .ok_or_else(|| CodecError::malformed("ZIP member frame storage overflows"))?;
        ctx.retain_bytes(u64_from_index(bytes), STORAGE)?;
        let mut order = ctx.alloc_filled(count, 0_usize, STORAGE)?;
        for (index, value) in order.iter_mut().enumerate() {
            *value = index;
        }
        order.sort_unstable_by_key(|index| self.entries[*index].header_start);
        let mut frames = ctx.alloc_filled(count, None, STORAGE)?;
        for (position, index) in order.iter().copied().enumerate() {
            let entry = &self.entries[index];
            if position.checked_sub(1).is_some_and(|previous| {
                self.entries[order[previous]].header_start == entry.header_start
            }) {
                continue;
            }
            let limit = order
                .get(position + 1)
                .map_or(self.central_start, |next| self.entries[*next].header_start)
                .min(self.central_start);
            let Ok(data) = entry::local_data_boundary(
                self.root.window(),
                entry.header_start,
                limit,
                entry.compressed_size,
            ) else {
                continue;
            };
            let end = data
                .checked_add(entry.compressed_size)
                .ok_or_else(|| CodecError::malformed("ZIP member payload boundary overflows"))?;
            frames[index] = Some(MemberFrame {
                archive: self.root,
                index,
                header: self.member_view(entry.header_start, data)?,
                payload: self.member_view(data, end)?,
            });
        }
        Ok(frames)
    }

    /// Parses a framed local header and returns the payload it actually admits.
    ///
    /// # Errors
    /// Rejects foreign frames, invalid signatures and inconsistent local boundaries.
    pub fn read_member_header(
        &self,
        ctx: &DecodeContext<'a>,
        frame: MemberFrame<'a>,
    ) -> Result<MemberPayload<'a>, CodecError> {
        let entry = self.frame_entry(ctx, &frame)?;
        let end = entry::local_data_start(
            self.root.window(),
            entry.header_start,
            self.central_start,
            entry.compressed_size,
        )
        .map_err(|error| CodecError::malformed(format_args!("{}: {error}", entry.name)))?;
        let expected = self.member_view(entry.header_start, end)?;
        if expected.location() != frame.header.location() || expected.end() != frame.header.end() {
            return Err(CodecError::malformed(
                "ZIP local header changed its framed boundary",
            ));
        }
        Ok(MemberPayload(frame))
    }

    /// Opens the compressed member admitted by its local-header callback.
    ///
    /// # Errors
    /// Rejects foreign frames, unsupported compression, corrupt data and budget failures.
    pub fn open_member(
        &self,
        ctx: &DecodeContext<'a>,
        payload: MemberPayload<'a>,
    ) -> Result<View<'a>, CodecError> {
        let entry = self.frame_entry(ctx, &payload.0)?;
        let view = payload.view();
        self.open_payload(
            ctx,
            entry,
            ByteRange {
                start: u64_from_index(view.start()),
                end: u64_from_index(view.end()),
            },
        )
    }

    fn frame_entry(
        &self,
        ctx: &DecodeContext<'a>,
        frame: &MemberFrame<'a>,
    ) -> Result<&EntryRecord, CodecError> {
        ctx.capture_part_extent(self.root)?;
        ctx.capture_part_extent(frame.archive)?;
        if self.root.location() != frame.archive.location()
            || self.root.end() != frame.archive.end()
        {
            return Err(CodecError::malformed(
                "ZIP member frame belongs to another archive",
            ));
        }
        self.entries
            .get(frame.index)
            .ok_or_else(|| CodecError::malformed("ZIP member frame has no declaration"))
    }

    fn member_view(&self, start: u64, end: u64) -> Result<View<'a>, CodecError> {
        let absolute = |offset| {
            usize::try_from(offset)
                .ok()
                .and_then(|offset| self.root.start().checked_add(offset))
        };
        absolute(start)
            .zip(absolute(end))
            .and_then(|(start, end)| self.root.child(start, end))
            .ok_or_else(|| CodecError::malformed("ZIP member frame escapes its actual archive"))
    }
}

#[cfg(test)]
mod tests;
