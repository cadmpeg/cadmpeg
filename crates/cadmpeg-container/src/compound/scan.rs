// SPDX-License-Identifier: Apache-2.0
//! Selective compound streams for bounded feature scans.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::scan::{ScanBudget, ScanReader, StopReason};

use super::{
    parse_directory, CompoundVersion, DirectoryKind, DirectorySlot, END_OF_CHAIN, FREE_SECTOR,
    MAGIC,
};

fn malformed(what: &str) -> StopReason {
    StopReason::Malformed(what.into())
}

/// One structurally reached compound stream.
#[derive(Debug)]
pub struct ScanCompoundEntry {
    /// Source directory slot.
    pub directory_id: usize,
    /// Exact source path.
    pub path: String,
    /// Source size claim.
    pub declared_size: u64,
}

/// Selective FAT and mini-FAT access under one scan budget.
pub struct ScanCompound<'a, 'b> {
    source: View<'a>,
    budget: &'b ScanBudget<'b>,
    sector_size: usize,
    sector_count: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    directory: Vec<DirectorySlot>,
    entries: Vec<ScanCompoundEntry>,
}

impl<'a, 'b> ScanCompound<'a, 'b> {
    /// Reads compound navigation tables and reached stream names.
    pub fn new(source: View<'a>, budget: &'b ScanBudget<'b>) -> Result<Self, StopReason> {
        budget.depth(1)?;
        let mut acquisition = ScanReader::source(source, budget);
        let mut header = acquisition.take(512)?;
        if header.req_take(8)? != MAGIC {
            return Err(malformed("CFB identity"));
        }
        header.req_take(16)?;
        header.req_u16_le()?;
        let major = header.req_u16_le()?;
        if header.req_u16_le()? != 0xfffe {
            return Err(malformed("CFB byte order"));
        }
        let shift = header.req_u16_le()?;
        let version = CompoundVersion::from_header(major, shift)
            .ok_or_else(|| StopReason::Unsupported("CFB sector version".into()))?;
        if header.req_u16_le()? != 6 {
            return Err(malformed("CFB mini sector size"));
        }
        header.req_take(6)?;
        header.req_u32_le()?;
        let fat_count =
            usize::try_from(header.req_u32_le()?).map_err(|_| StopReason::RetainedBytes)?;
        let directory_start = header.req_u32_le()?;
        header.req_u32_le()?;
        if header.req_u32_le()? != 4096 {
            return Err(malformed("CFB mini stream cutoff"));
        }
        let mini_fat_start = header.req_u32_le()?;
        let mini_fat_count =
            usize::try_from(header.req_u32_le()?).map_err(|_| StopReason::RetainedBytes)?;
        let mut difat = header.req_u32_le()?;
        let difat_start = difat;
        let difat_count = header.req_u32_le()?;
        let sector_size = version.sector_size();
        let sector_count = source
            .window()
            .len()
            .checked_sub(sector_size)
            .ok_or_else(|| malformed("CFB header sector"))?
            .div_ceil(sector_size);
        if fat_count > sector_count
            || mini_fat_count > sector_count
            || u64::from(difat_count) > u64_from_index(sector_count)
        {
            return Err(malformed("CFB structural count"));
        }
        let mut fat_ids = Vec::new();
        for _ in 0..109 {
            budget.check()?;
            let id = header.req_u32_le()?;
            if id != FREE_SECTOR {
                budget.retain(8)?;
                fat_ids.push(id);
            }
        }
        let mut result = Self {
            source,
            budget,
            sector_size,
            sector_count,
            fat: Vec::new(),
            mini_fat: Vec::new(),
            directory: Vec::new(),
            entries: Vec::new(),
        };
        let mut seen = BTreeSet::new();
        for _ in 0..difat_count {
            budget.check()?;
            budget.retain(64)?;
            if !seen.insert(difat) {
                return Err(malformed("CFB cyclic DIFAT"));
            }
            let mut body = result.sector(difat)?;
            for _ in 0..sector_size / 4 - 1 {
                budget.check()?;
                let id = body.req_u32_le()?;
                if id != FREE_SECTOR {
                    budget.retain(8)?;
                    fat_ids.push(id);
                }
            }
            difat = body.req_u32_le()?;
        }
        if fat_ids.len() != fat_count
            || super::difat_end(
                difat_start,
                usize::try_from(difat_count).map_err(|_| malformed("CFB DIFAT count"))?,
                difat,
                fat_count,
                fat_ids.len(),
                sector_size,
            )
            .is_none()
        {
            return Err(malformed("CFB FAT count"));
        }
        let mut fat_seen = BTreeSet::new();
        for id in fat_ids {
            budget.retain(64)?;
            if !fat_seen.insert(id) {
                return Err(malformed("CFB repeated FAT sector"));
            }
            let mut body = result.sector(id)?;
            while body.remaining() > 0 {
                budget.check()?;
                budget.retain(8)?;
                result.fat.push(body.req_u32_le()?);
            }
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let ctx = DecodeContext::new(
            &arena,
            &policy,
            cadmpeg_core::decode::DecodeDepth::Container,
        );
        for id in result.chain(directory_start, None)? {
            let mut body = result.sector(id)?;
            while body.remaining() > 0 {
                budget.check()?;
                budget.retain(512)?;
                let record = body.req_take_child(128)?;
                let mut parsed = parse_directory(&ctx, record.window(), version)
                    .map_err(|error| malformed(&error.to_string()))?;
                result.directory.push(
                    parsed
                        .pop()
                        .ok_or_else(|| malformed("CFB directory record"))?,
                );
            }
        }
        if mini_fat_count != 0 {
            for id in result.chain(mini_fat_start, Some(mini_fat_count))? {
                let mut body = result.sector(id)?;
                while body.remaining() > 0 {
                    budget.check()?;
                    budget.retain(8)?;
                    result.mini_fat.push(body.req_u32_le()?);
                }
            }
        }
        let root = result
            .directory
            .first()
            .and_then(DirectorySlot::live)
            .ok_or_else(|| malformed("CFB root directory"))?;
        if root.kind != DirectoryKind::Root {
            return Err(malformed("CFB root kind"));
        }
        let mut pending = vec![(root.child, String::new(), 1_u64)];
        let mut reached = BTreeSet::new();
        let mut paths = BTreeSet::new();
        while let Some((id, parent, depth)) = pending.pop() {
            budget.check()?;
            if id == FREE_SECTOR {
                continue;
            }
            budget.depth(depth)?;
            budget.retain(256)?;
            if !reached.insert(id) {
                return Err(malformed("CFB directory reference cycle"));
            }
            let slot = usize::try_from(id).map_err(|_| malformed("CFB directory id"))?;
            let entry = result
                .directory
                .get(slot)
                .and_then(DirectorySlot::live)
                .ok_or_else(|| malformed("CFB directory reference"))?;
            budget.retain(
                u64_from_index(parent.len())
                    .checked_mul(3)
                    .and_then(|n| n.checked_add(192))
                    .ok_or(StopReason::RetainedBytes)?,
            )?;
            let path = if parent.is_empty() {
                entry.name.as_str().to_owned()
            } else {
                format!("{parent}/{}", entry.name.as_str())
            };
            if !paths.insert(path.clone()) {
                return Err(malformed("CFB duplicate source path"));
            }
            pending.push((entry.right, parent.clone(), depth));
            pending.push((entry.left, parent, depth));
            if entry.kind == DirectoryKind::Stream {
                result.entries.push(ScanCompoundEntry {
                    directory_id: slot,
                    path,
                    declared_size: entry.size,
                });
            } else if entry.kind == DirectoryKind::Storage {
                pending.push((
                    entry.child,
                    path,
                    depth.checked_add(1).ok_or(StopReason::NestingDepth)?,
                ));
            }
        }
        result.entries.sort_by_key(|entry| entry.directory_id);
        Ok(result)
    }

    fn sector(&self, id: u32) -> Result<View<'a>, StopReason> {
        self.budget.check()?;
        let index = usize::try_from(id).map_err(|_| malformed("CFB sector id"))?;
        if index >= self.sector_count {
            return Err(malformed("CFB sector outside source"));
        }
        let at = index
            .checked_add(1)
            .and_then(|n| n.checked_mul(self.sector_size))
            .and_then(|n| n.checked_add(self.source.start()))
            .ok_or_else(|| malformed("CFB sector offset"))?;
        let mut reader = ScanReader::source(self.source, self.budget);
        reader.seek(at)?;
        reader.take(self.sector_size)
    }

    fn chain(&self, start: u32, count: Option<usize>) -> Result<Vec<u32>, StopReason> {
        let mut current = start;
        let mut seen = BTreeSet::new();
        let mut chain = Vec::new();
        while current != END_OF_CHAIN {
            self.budget.check()?;
            self.budget.retain(64)?;
            if chain.len() >= self.sector_count || !seen.insert(current) {
                return Err(malformed("CFB sector chain cycle"));
            }
            let index = usize::try_from(current).map_err(|_| malformed("CFB FAT index"))?;
            if index >= self.sector_count {
                return Err(malformed("CFB FAT range"));
            }
            chain.push(current);
            current = *self
                .fat
                .get(index)
                .ok_or_else(|| malformed("CFB FAT link"))?;
        }
        if count.is_some_and(|count| count != chain.len()) {
            return Err(malformed("CFB chain length"));
        }
        Ok(chain)
    }

    /// Structurally reached streams in source directory order.
    pub fn entries(&self) -> &[ScanCompoundEntry] {
        &self.entries
    }

    /// Reads one selected stream; every source sector is admitted separately.
    pub fn open(&self, entry: &ScanCompoundEntry) -> Result<Vec<u8>, StopReason> {
        self.read_prefix(entry, None)
    }

    /// Reads at most `length` bytes to select a metadata stream from its header.
    pub fn prefix(&self, entry: &ScanCompoundEntry, length: usize) -> Result<Vec<u8>, StopReason> {
        self.read_prefix(entry, Some(length))
    }

    fn read_prefix(
        &self,
        entry: &ScanCompoundEntry,
        limit: Option<usize>,
    ) -> Result<Vec<u8>, StopReason> {
        self.budget.check()?;
        let live = self
            .directory
            .get(entry.directory_id)
            .and_then(DirectorySlot::live)
            .ok_or_else(|| malformed("CFB selected stream"))?;
        let size = usize::try_from(live.size).map_err(|_| StopReason::RetainedBytes)?;
        let take_size = limit.map_or(size, |limit| size.min(limit));
        let mut bytes = Vec::new();
        if take_size == 0 {
            return Ok(bytes);
        }
        if size >= 4096 {
            for id in self.chain(live.start_sector, Some(size.div_ceil(self.sector_size)))? {
                let body = self.sector(id)?;
                self.append(&mut bytes, body, take_size)?;
                if bytes.len() == take_size {
                    break;
                }
            }
        } else {
            let root = self
                .directory
                .first()
                .and_then(DirectorySlot::live)
                .ok_or_else(|| malformed("CFB mini stream root"))?;
            let root_size = usize::try_from(root.size).map_err(|_| StopReason::RetainedBytes)?;
            let root_chain = self.chain(
                root.start_sector,
                Some(root_size.div_ceil(self.sector_size)),
            )?;
            let mut current = live.start_sector;
            let mut seen = BTreeSet::new();
            while bytes.len() < take_size {
                self.budget.check()?;
                self.budget.retain(64)?;
                if !seen.insert(current) {
                    return Err(malformed("CFB mini sector cycle"));
                }
                let index =
                    usize::try_from(current).map_err(|_| malformed("CFB mini sector index"))?;
                let offset = index
                    .checked_mul(64)
                    .ok_or_else(|| malformed("CFB mini sector offset"))?;
                if offset.checked_add(64).is_none_or(|end| end > root_size) {
                    return Err(malformed("CFB mini sector extent"));
                }
                let id = *root_chain
                    .get(offset / self.sector_size)
                    .ok_or_else(|| malformed("CFB mini stream sector"))?;
                let at = usize::try_from(id)
                    .ok()
                    .and_then(|id| id.checked_add(1))
                    .and_then(|id| id.checked_mul(self.sector_size))
                    .and_then(|at| at.checked_add(offset % self.sector_size))
                    .and_then(|at| at.checked_add(self.source.start()))
                    .ok_or_else(|| malformed("CFB mini sector position"))?;
                let mut reader = ScanReader::source(self.source, self.budget);
                reader.seek(at)?;
                self.append(&mut bytes, reader.take(64)?, take_size)?;
                current = *self
                    .mini_fat
                    .get(index)
                    .ok_or_else(|| malformed("CFB mini FAT link"))?;
            }
            if take_size == size && current != END_OF_CHAIN {
                return Err(malformed("CFB mini chain length"));
            }
        }
        Ok(bytes)
    }

    fn append(&self, bytes: &mut Vec<u8>, body: View<'_>, size: usize) -> Result<(), StopReason> {
        let count = body.window().len().min(size - bytes.len());
        self.budget.retain(u64_from_index(count))?;
        bytes
            .try_reserve_exact(count)
            .map_err(|_| StopReason::RetainedBytes)?;
        bytes.extend_from_slice(&body.window()[..count]);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
