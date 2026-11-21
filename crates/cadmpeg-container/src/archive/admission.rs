// SPDX-License-Identifier: Apache-2.0
//! Caller-specific ZIP footer resource admission.
use crate::layout::{end_record, zip64_end, zip64_locator};
use cadmpeg_core::decode::View;

impl super::ArchiveSnapshot<'_> {
    /// Admits a single-disk ZIP with a local header within caller directory limits.
    pub fn admit_directory(root: View<'_>, members_limit: u64, directory_limit: u64) -> Option<()> {
        let bytes = root.window();
        if !bytes.starts_with(b"PK\x03\x04") {
            return None;
        }
        let start = bytes.len()
            - bytes
                .len()
                .min(end_record::LEN + usize::from(u16::MAX) + zip64_locator::LEN);
        let tail = bytes.get(start..)?;
        let offset = tail.windows(4).rposition(|w| w == b"PK\x05\x06")?;
        let end = root.child(start.checked_add(offset)?, bytes.len())?;
        let word = |at| View::u16_le_at(end.window(), at);
        let dword = |at| View::u32_le_at(end.window(), at);
        if end.window().len()
            != end_record::LEN.checked_add(usize::from(word(end_record::COMMENT_LENGTH)?))?
            || word(end_record::DISK)? != 0
            || word(end_record::DIRECTORY_DISK)? != 0
        {
            return None;
        }
        let locator = offset
            .checked_sub(zip64_locator::LEN)
            .and_then(|at| root.child(start + at, start + offset));
        let (members, size, position) = if let Some(locator) =
            locator.filter(|l| l.window().starts_with(b"PK\x06\x07"))
        {
            let disk = View::u32_le_at(locator.window(), zip64_locator::END_RECORD_DISK)?;
            let at = View::u64_le_at(locator.window(), zip64_locator::END_RECORD_POSITION)?;
            let disks = View::u32_le_at(locator.window(), zip64_locator::DISKS)?;
            if disk != 0 || disks != 1 {
                return None;
            }
            let at = usize::try_from(at).ok()?;
            let record = root.child(at, at.checked_add(zip64_end::LEN)?)?;
            if !record.window().starts_with(b"PK\x06\x06")
                || View::u32_le_at(record.window(), zip64_end::DISK)? != 0
                || View::u32_le_at(record.window(), zip64_end::DIRECTORY_DISK)? != 0
            {
                return None;
            }
            let value = |at| View::u64_le_at(record.window(), at);
            let size = value(zip64_end::RECORD_SIZE)?;
            if !(cadmpeg_core::decode::u64_from_index(zip64_end::LEN - zip64_end::PRODUCER_VERSION)
                ..=directory_limit)
                .contains(&size)
                || at
                    .checked_add(zip64_end::PRODUCER_VERSION)?
                    .checked_add(usize::try_from(size).ok()?)?
                    > bytes.len()
                || value(zip64_end::DISK_ENTRIES)? != value(zip64_end::ENTRIES)?
            {
                return None;
            }
            (
                value(zip64_end::ENTRIES)?,
                value(zip64_end::DIRECTORY_SIZE)?,
                value(zip64_end::DIRECTORY_POSITION)?,
            )
        } else {
            if word(end_record::DISK_ENTRIES)? != word(end_record::ENTRIES)? {
                return None;
            }
            (
                u64::from(word(end_record::ENTRIES)?),
                u64::from(dword(end_record::DIRECTORY_SIZE)?),
                u64::from(dword(end_record::DIRECTORY_POSITION)?),
            )
        };
        if members > members_limit
            || size > directory_limit
            || position.checked_add(size)? > cadmpeg_core::decode::u64_from_index(bytes.len())
        {
            return None;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests;
