// SPDX-License-Identifier: Apache-2.0
//! Lazy, budgeted Microsoft Compound File Binary (CFB) snapshots.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{ByteRange, DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const DIFAT_SECTOR: u32 = 0xffff_fffc;
const NO_STREAM: u32 = 0xffff_ffff;

/// Stable directory identity for a CFB entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompoundEntryId(u32);

impl CompoundEntryId {
    /// Returns the CFB directory-entry index.
    pub const fn directory_id(self) -> u32 {
        self.0
    }
}

/// Stable identity for a CFB storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompoundStorageId(CompoundEntryId);

impl CompoundStorageId {
    /// Returns the CFB directory-entry index.
    pub const fn directory_id(self) -> u32 {
        self.0.directory_id()
    }
}

/// Stable identity for a CFB stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompoundStreamId(CompoundEntryId);

impl CompoundStreamId {
    /// Returns the CFB directory-entry index.
    pub const fn directory_id(self) -> u32 {
        self.0.directory_id()
    }
}

/// CFB allocation mechanism for a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundAllocation {
    /// Regular sectors addressed through the FAT.
    Regular,
    /// 64-byte mini sectors addressed through the mini FAT and root mini stream.
    Mini,
}

impl CompoundAllocation {
    /// Returns the stable summary label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Mini => "mini",
        }
    }
}

/// One storage in the CFB directory hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundStorageEntry {
    id: CompoundStorageId,
    path: String,
}

impl CompoundStorageEntry {
    /// Returns the stable storage identity.
    pub const fn id(&self) -> CompoundStorageId {
        self.id
    }

    /// Returns the exact hierarchy path with source spelling preserved.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// One stream in the CFB directory hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundStreamEntry {
    id: CompoundStreamId,
    path: String,
    logical_size: u64,
    start_sector: u32,
    allocation: CompoundAllocation,
    chain: Vec<u32>,
}

impl CompoundStreamEntry {
    /// Returns the stable stream identity.
    pub const fn id(&self) -> CompoundStreamId {
        self.id
    }

    /// Returns the exact hierarchy path with source spelling preserved.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the logical stream size without allocation padding.
    pub const fn logical_size(&self) -> u64 {
        self.logical_size
    }

    /// Returns the first regular or mini sector identifier.
    pub const fn start_sector(&self) -> u32 {
        self.start_sector
    }

    /// Returns the stream allocation mechanism.
    pub const fn allocation(&self) -> CompoundAllocation {
        self.allocation
    }
}

/// A typed CFB directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompoundEntry {
    /// A storage, including no stream-opening operation.
    Storage(CompoundStorageEntry),
    /// A byte stream that can be passed to [`CompoundSnapshot::open`].
    Stream(CompoundStreamEntry),
}

impl CompoundEntry {
    /// Returns the exact hierarchy path.
    pub fn path(&self) -> &str {
        match self {
            Self::Storage(entry) => entry.path(),
            Self::Stream(entry) => entry.path(),
        }
    }

    /// Returns the CFB directory-entry index.
    pub const fn directory_id(&self) -> u32 {
        match self {
            Self::Storage(entry) => entry.id().directory_id(),
            Self::Stream(entry) => entry.id().directory_id(),
        }
    }
}

/// One exact physical range in a CFB file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundPhysicalSpan {
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
    /// CFB structural role.
    pub role: String,
    /// Owning entry path, when the range stores stream data.
    pub entry: Option<String>,
}

#[derive(Debug, Clone)]
struct DirectoryEntry {
    name: String,
    object_type: u8,
    color: u8,
    left: u32,
    right: u32,
    child: u32,
    start_sector: u32,
    size: u64,
}

#[derive(Debug)]
struct CompoundState {
    major_version: u16,
    sector_size: usize,
    mini_sector_size: usize,
    mini_stream_cutoff: u64,
    sector_count: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    directory: Vec<DirectoryEntry>,
    directory_chain: Vec<u32>,
    mini_fat_chain: Vec<u32>,
    root_mini_chain: Vec<u32>,
    fat_sectors: BTreeSet<u32>,
    difat_sectors: BTreeSet<u32>,
}

/// Parsed CFB navigation state over one decode-session root view.
#[derive(Debug)]
pub struct CompoundSnapshot<'a> {
    root: View<'a>,
    parsed: CompoundState,
    entries: Vec<CompoundEntry>,
    by_path: BTreeMap<String, usize>,
    streams_by_id: BTreeMap<CompoundStreamId, usize>,
}

impl<'a> CompoundSnapshot<'a> {
    /// Parses and validates the complete CFB structure without opening streams.
    pub fn new(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let parsed = CompoundState::parse(ctx, root.window())?;
        let entries = parsed.build_entries(ctx)?;
        let mut by_path = BTreeMap::new();
        let mut streams_by_id = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let key = path_key(entry.path());
            if by_path.insert(key, index).is_some() {
                return malformed(format!("duplicate CFB path {}", entry.path()));
            }
            if let CompoundEntry::Stream(stream) = entry {
                streams_by_id.insert(stream.id(), index);
            }
        }
        let retained_bytes = parsed
            .fat
            .len()
            .saturating_add(parsed.mini_fat.len())
            .saturating_add(parsed.directory_chain.len())
            .saturating_add(parsed.mini_fat_chain.len())
            .saturating_add(parsed.root_mini_chain.len())
            .saturating_mul(std::mem::size_of::<u32>())
            .saturating_add(
                entries
                    .iter()
                    .map(|entry| entry.path().len())
                    .sum::<usize>(),
            );
        ctx.charge_retained(
            retained_bytes as u64,
            "retain CFB snapshot indexes",
            Some(root.location()),
        )?;
        ctx.charge_collection_items(
            parsed.directory.len().saturating_add(entries.len()) as u64,
            "index CFB directory entries",
        )?;
        Ok(Self {
            root,
            parsed,
            entries,
            by_path,
            streams_by_id,
        })
    }

    /// Returns the CFB major version.
    pub const fn major_version(&self) -> u16 {
        self.parsed.major_version
    }

    /// Returns the regular-sector size.
    pub const fn sector_size(&self) -> usize {
        self.parsed.sector_size
    }

    /// Returns entries in stable directory traversal order.
    pub fn entries(&self) -> &[CompoundEntry] {
        &self.entries
    }

    /// Finds an entry by a case-insensitive CFB path key.
    pub fn entry(&self, path: &str) -> Option<&CompoundEntry> {
        self.by_path
            .get(&path_key(path))
            .map(|index| &self.entries[*index])
    }

    /// Finds a stream by a case-insensitive CFB path key.
    pub fn stream(&self, path: &str) -> Option<&CompoundStreamEntry> {
        match self.entry(path) {
            Some(CompoundEntry::Stream(entry)) => Some(entry),
            _ => None,
        }
    }

    /// Finds a stream by stable directory identity.
    pub fn stream_by_id(&self, id: CompoundStreamId) -> Option<&CompoundStreamEntry> {
        self.streams_by_id
            .get(&id)
            .and_then(|index| match &self.entries[*index] {
                CompoundEntry::Stream(entry) => Some(entry),
                CompoundEntry::Storage(_) => None,
            })
    }

    /// Opens one stream as a borrowed contiguous run or a budgeted joined view.
    pub fn open(
        &self,
        ctx: &DecodeContext<'a>,
        entry: &CompoundStreamEntry,
    ) -> Result<View<'a>, CodecError> {
        if self.stream_by_id(entry.id()).is_none() {
            return malformed("CFB stream handle does not belong to this snapshot");
        }
        if entry.logical_size == 0 {
            return ctx.register_slice(self.root, ByteRange { start: 0, end: 0 });
        }
        let logical_size = usize::try_from(entry.logical_size)
            .map_err(|_| CodecError::Malformed("CFB stream size does not fit memory".into()))?;
        let views = match entry.allocation {
            CompoundAllocation::Regular => entry
                .chain
                .iter()
                .map(|&sector| self.regular_sector_view(sector))
                .collect::<Result<Vec<_>, _>>()?,
            CompoundAllocation::Mini => entry
                .chain
                .iter()
                .map(|&sector| self.mini_sector_view(sector))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let mut opened = if physically_contiguous(&views) {
            let first = views.first().ok_or_else(|| {
                CodecError::Malformed(format!("empty allocation chain for {}", entry.path))
            })?;
            let last = views.last().expect("non-empty chain");
            self.root.child(first.start(), last.end()).ok_or_else(|| {
                CodecError::Malformed("CFB contiguous stream range escapes input".into())
            })?
        } else {
            ctx.concat_views(&views)?
        };
        opened = opened
            .child(opened.start(), opened.start() + logical_size)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "CFB stream {} is shorter than declared",
                    entry.path
                ))
            })?;
        Ok(opened)
    }

    /// Builds generic hierarchy summaries using a codec-owned classifier.
    pub fn container_entries(
        &self,
        classify: impl Fn(&CompoundEntry) -> &'static str,
    ) -> Vec<ContainerEntry> {
        self.entries
            .iter()
            .map(|entry| {
                let mut attributes = BTreeMap::new();
                attributes.insert("directory_id".into(), entry.directory_id().to_string());
                match entry {
                    CompoundEntry::Storage(_) => ContainerEntry {
                        name: entry.path().into(),
                        role: classify(entry).into(),
                        compression: "storage".into(),
                        compressed_size: 0,
                        uncompressed_size: 0,
                        attributes,
                    },
                    CompoundEntry::Stream(stream) => {
                        attributes.insert("allocation".into(), stream.allocation.label().into());
                        attributes.insert("start_sector".into(), stream.start_sector.to_string());
                        ContainerEntry {
                            name: stream.path.clone(),
                            role: classify(entry).into(),
                            compression: "stored".into(),
                            compressed_size: stream.logical_size,
                            uncompressed_size: stream.logical_size,
                            attributes,
                        }
                    }
                }
            })
            .collect()
    }

    /// Partitions every physical input byte by CFB structural role.
    pub fn physical_ledger(&self) -> Result<Vec<CompoundPhysicalSpan>, CodecError> {
        let mut structural = BTreeMap::new();
        for &sector in &self.parsed.fat_sectors {
            structural.insert(sector, "FAT");
        }
        for &sector in &self.parsed.difat_sectors {
            structural.insert(sector, "DIFAT");
        }
        for &sector in &self.parsed.directory_chain {
            structural.insert(sector, "directory");
        }
        for &sector in &self.parsed.mini_fat_chain {
            structural.insert(sector, "mini FAT");
        }
        let mut regular = BTreeMap::new();
        let mut mini = BTreeMap::new();
        for entry in &self.entries {
            let CompoundEntry::Stream(stream) = entry else {
                continue;
            };
            let width = match stream.allocation {
                CompoundAllocation::Regular => self.parsed.sector_size,
                CompoundAllocation::Mini => self.parsed.mini_sector_size,
            };
            let mut remaining = stream.logical_size;
            for &sector in &stream.chain {
                let payload = remaining.min(width as u64) as usize;
                remaining = remaining.saturating_sub(payload as u64);
                match stream.allocation {
                    CompoundAllocation::Regular => {
                        regular.insert(sector, (stream.path.clone(), payload));
                    }
                    CompoundAllocation::Mini => {
                        mini.insert(sector, (stream.path.clone(), payload));
                    }
                }
            }
        }
        let mut spans = vec![CompoundPhysicalSpan {
            start: 0,
            end: self.parsed.sector_size as u64,
            role: "header".into(),
            entry: None,
        }];
        let root_size = self.parsed.directory[0].size;
        let root_sectors = self
            .parsed
            .root_mini_chain
            .iter()
            .enumerate()
            .map(|(ordinal, sector)| (*sector, ordinal))
            .collect::<BTreeMap<_, _>>();
        for index in 0..self.parsed.sector_count {
            let start =
                self.parsed
                    .sector_size
                    .checked_add(index.checked_mul(self.parsed.sector_size).ok_or_else(|| {
                        CodecError::Malformed("CFB ledger offset overflow".into())
                    })?)
                    .ok_or_else(|| CodecError::Malformed("CFB ledger offset overflow".into()))?
                    as u64;
            let sector = u32::try_from(index)
                .map_err(|_| CodecError::Malformed("CFB sector id exceeds u32".into()))?;
            if let Some(role) = structural.get(&sector) {
                push_span(&mut spans, start, self.parsed.sector_size, role, None);
            } else if let Some((entry, payload)) = regular.get(&sector) {
                push_span(
                    &mut spans,
                    start,
                    *payload,
                    "regular stream payload",
                    Some(entry.clone()),
                );
                push_span(
                    &mut spans,
                    start + *payload as u64,
                    self.parsed.sector_size - *payload,
                    "padding",
                    Some(entry.clone()),
                );
            } else if let Some(root_ordinal) = root_sectors.get(&sector) {
                for mini_ordinal in 0..self.parsed.sector_size / self.parsed.mini_sector_size {
                    let logical_mini = root_ordinal
                        .checked_mul(self.parsed.sector_size / self.parsed.mini_sector_size)
                        .and_then(|base| base.checked_add(mini_ordinal))
                        .ok_or_else(|| {
                            CodecError::Malformed("CFB mini-sector id overflow".into())
                        })?;
                    let mini_start = start + (mini_ordinal * self.parsed.mini_sector_size) as u64;
                    let root_offset = (logical_mini * self.parsed.mini_sector_size) as u64;
                    let mapped = root_size
                        .saturating_sub(root_offset)
                        .min(self.parsed.mini_sector_size as u64)
                        as usize;
                    let logical_mini = u32::try_from(logical_mini).map_err(|_| {
                        CodecError::Malformed("CFB mini-sector id exceeds u32".into())
                    })?;
                    if let Some((entry, payload)) = mini.get(&logical_mini) {
                        push_span(
                            &mut spans,
                            mini_start,
                            *payload,
                            "mini stream payload",
                            Some(entry.clone()),
                        );
                        push_span(
                            &mut spans,
                            mini_start + *payload as u64,
                            self.parsed.mini_sector_size - *payload,
                            "padding",
                            Some(entry.clone()),
                        );
                    } else {
                        push_span(&mut spans, mini_start, mapped, "mini-stream padding", None);
                        push_span(
                            &mut spans,
                            mini_start + mapped as u64,
                            self.parsed.mini_sector_size - mapped,
                            "padding",
                            None,
                        );
                    }
                }
            } else {
                push_span(
                    &mut spans,
                    start,
                    self.parsed.sector_size,
                    "unallocated sector",
                    None,
                );
            }
        }
        Ok(spans)
    }

    fn regular_sector_view(&self, sector: u32) -> Result<View<'a>, CodecError> {
        let (start, end) = sector_range(self.parsed.sector_size, self.parsed.sector_count, sector)?;
        self.root
            .child(start, end)
            .ok_or_else(|| CodecError::Malformed("CFB sector escapes input".into()))
    }

    fn mini_sector_view(&self, mini_sector: u32) -> Result<View<'a>, CodecError> {
        let offset = usize::try_from(mini_sector)
            .ok()
            .and_then(|id| id.checked_mul(self.parsed.mini_sector_size))
            .ok_or_else(|| CodecError::Malformed("CFB mini-sector offset overflow".into()))?;
        let regular_ordinal = offset / self.parsed.sector_size;
        let within = offset % self.parsed.sector_size;
        let &regular_sector = self
            .parsed
            .root_mini_chain
            .get(regular_ordinal)
            .ok_or_else(|| {
                CodecError::Malformed("CFB mini sector escapes the root mini stream".into())
            })?;
        let sector = self.regular_sector_view(regular_sector)?;
        sector
            .child(
                sector.start() + within,
                sector.start() + within + self.parsed.mini_sector_size,
            )
            .ok_or_else(|| {
                CodecError::Malformed("CFB mini sector crosses a regular-sector boundary".into())
            })
    }
}

impl CompoundState {
    fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.get(..8) != Some(&MAGIC) {
            return malformed("input is not a CFB file");
        }
        let field = |offset, what| {
            le_u32(bytes, offset)
                .ok_or_else(|| CodecError::Malformed(format!("truncated CFB {what}")))
        };
        if bytes.get(8..24) != Some(&[0; 16])
            || le_u16(bytes, 24) != Some(0x003e)
            || le_u16(bytes, 28) != Some(0xfffe)
        {
            return malformed("invalid CFB header identity or byte order");
        }
        let major_version = le_u16(bytes, 26)
            .ok_or_else(|| CodecError::Malformed("truncated CFB version".into()))?;
        let sector_shift = le_u16(bytes, 30)
            .ok_or_else(|| CodecError::Malformed("truncated CFB sector shift".into()))?;
        if !matches!((major_version, sector_shift), (3, 9) | (4, 12))
            || le_u16(bytes, 32) != Some(6)
            || bytes.get(34..40) != Some(&[0; 6])
        {
            return malformed("unsupported or invalid CFB sector layout");
        }
        let sector_size = 1usize
            .checked_shl(u32::from(sector_shift))
            .ok_or_else(|| CodecError::Malformed("CFB sector size overflow".into()))?;
        if bytes.len() < sector_size || !(bytes.len() - sector_size).is_multiple_of(sector_size) {
            return malformed("CFB input does not end on a sector boundary");
        }
        let sector_count = (bytes.len() - sector_size) / sector_size;
        let directory_sector_count = usize::try_from(field(40, "directory sector count")?)
            .map_err(|_| {
                CodecError::Malformed("CFB directory sector count does not fit memory".into())
            })?;
        let fat_count = usize::try_from(field(44, "FAT count")?)
            .map_err(|_| CodecError::Malformed("CFB FAT count does not fit memory".into()))?;
        let directory_start = field(48, "directory start")?;
        let transaction_signature = field(52, "transaction signature")?;
        let mini_stream_cutoff = u64::from(field(56, "mini-stream cutoff")?);
        let mini_fat_start = field(60, "mini FAT start")?;
        let mini_fat_count = usize::try_from(field(64, "mini FAT count")?)
            .map_err(|_| CodecError::Malformed("CFB mini FAT count does not fit memory".into()))?;
        let difat_start = field(68, "DIFAT start")?;
        let difat_count = usize::try_from(field(72, "DIFAT count")?)
            .map_err(|_| CodecError::Malformed("CFB DIFAT count does not fit memory".into()))?;
        if (major_version == 3 && directory_sector_count != 0)
            || mini_stream_cutoff != 4096
            || fat_count > sector_count
            || difat_count > sector_count
            || transaction_signature != 0
        {
            return malformed("invalid CFB header counts or reserved fields");
        }
        ctx.charge_collection_items(
            (fat_count + difat_count) as u64,
            "parse CFB allocation tables",
        )?;
        let sector = |id| sector_slice(bytes, sector_size, sector_count, id);
        let mut fat_sectors = Vec::with_capacity(fat_count);
        let mut header_free_seen = false;
        for index in 0..109 {
            let id = field(76 + index * 4, "header DIFAT entry")?;
            if id == FREE_SECTOR {
                header_free_seen = true;
            } else {
                if header_free_seen {
                    return malformed("non-free CFB header DIFAT entry follows a free entry");
                }
                fat_sectors.push(id);
            }
        }
        let mut next_difat = difat_start;
        let difat_entries = sector_size / 4 - 1;
        let mut seen_difat = BTreeSet::new();
        for _ in 0..difat_count {
            if next_difat >= sector_count as u32 || !seen_difat.insert(next_difat) {
                return malformed("CFB DIFAT chain is cyclic or out of range");
            }
            let data = sector(next_difat)
                .ok_or_else(|| CodecError::Malformed("CFB DIFAT sector is absent".into()))?;
            let mut free_seen = false;
            for index in 0..difat_entries {
                let id = le_u32(data, index * 4)
                    .ok_or_else(|| CodecError::Malformed("truncated CFB DIFAT sector".into()))?;
                if id == FREE_SECTOR {
                    free_seen = true;
                } else {
                    if free_seen {
                        return malformed("non-free CFB DIFAT entry follows a free entry");
                    }
                    fat_sectors.push(id);
                }
            }
            next_difat = le_u32(data, difat_entries * 4)
                .ok_or_else(|| CodecError::Malformed("truncated CFB DIFAT link".into()))?;
        }
        if (difat_count == 0 && difat_start != END_OF_CHAIN)
            || (difat_count != 0 && next_difat != END_OF_CHAIN)
            || fat_sectors.len() != fat_count
            || fat_sectors.iter().any(|id| *id >= sector_count as u32)
        {
            return malformed("CFB DIFAT does not match its declared FAT count");
        }
        let fat_sector_set = fat_sectors.iter().copied().collect::<BTreeSet<_>>();
        if fat_sector_set.len() != fat_sectors.len() {
            return malformed("duplicate CFB FAT sector");
        }
        if !fat_sector_set.is_disjoint(&seen_difat) {
            return malformed("CFB sector has both FAT and DIFAT roles");
        }
        let mut fat = Vec::with_capacity(fat_count.saturating_mul(sector_size / 4));
        for &id in &fat_sectors {
            let data = sector(id)
                .ok_or_else(|| CodecError::Malformed("CFB FAT sector is absent".into()))?;
            fat.extend(
                data.chunks_exact(4)
                    .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte chunk"))),
            );
        }
        if fat.len() < sector_count {
            return malformed("CFB FAT does not address every physical sector");
        }
        if fat_sectors
            .iter()
            .any(|id| fat.get(*id as usize) != Some(&FAT_SECTOR))
            || seen_difat
                .iter()
                .any(|id| fat.get(*id as usize) != Some(&DIFAT_SECTOR))
        {
            return malformed("CFB allocation table sector has the wrong role marker");
        }
        let directory_expected = (major_version == 4).then_some(directory_sector_count);
        let directory_chain = chain(
            &fat,
            sector_count,
            directory_start,
            directory_expected,
            "directory",
        )?;
        let directory_bytes = join_sectors(bytes, sector_size, sector_count, &directory_chain)?;
        let directory = parse_directory(&directory_bytes, major_version)?;
        validate_root(&directory)?;
        let mini_fat_chain = if mini_fat_count == 0 {
            if mini_fat_start != END_OF_CHAIN {
                return malformed("empty CFB mini FAT has a start sector");
            }
            Vec::new()
        } else {
            chain(
                &fat,
                sector_count,
                mini_fat_start,
                Some(mini_fat_count),
                "mini FAT",
            )?
        };
        let mini_fat = join_sectors(bytes, sector_size, sector_count, &mini_fat_chain)?
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte chunk")))
            .collect::<Vec<_>>();
        let root = &directory[0];
        let root_sectors = usize::try_from(root.size)
            .map_err(|_| {
                CodecError::Malformed("CFB root mini-stream size does not fit memory".into())
            })?
            .div_ceil(sector_size);
        let root_mini_chain = if root.size == 0 {
            if !matches!(root.start_sector, END_OF_CHAIN | FREE_SECTOR) {
                return malformed("empty CFB root mini stream has an invalid start sector");
            }
            Vec::new()
        } else {
            chain(
                &fat,
                sector_count,
                root.start_sector,
                Some(root_sectors),
                "root mini stream",
            )?
        };
        let state = Self {
            major_version,
            sector_size,
            mini_sector_size: 64,
            mini_stream_cutoff,
            sector_count,
            fat,
            mini_fat,
            directory,
            directory_chain,
            mini_fat_chain,
            root_mini_chain,
            fat_sectors: fat_sector_set,
            difat_sectors: seen_difat,
        };
        state.validate_sector_ownership()?;
        Ok(state)
    }

    fn build_entries(&self, ctx: &DecodeContext<'_>) -> Result<Vec<CompoundEntry>, CodecError> {
        let mut output = Vec::new();
        let mut reached = BTreeSet::new();
        self.walk_tree(ctx, self.directory[0].child, "", &mut reached, &mut output)?;
        if self
            .directory
            .iter()
            .enumerate()
            .skip(1)
            .any(|(id, entry)| entry.object_type != 0 && !reached.contains(&(id as u32)))
        {
            return malformed("CFB directory contains an unreachable live entry");
        }
        Ok(output)
    }

    fn walk_tree(
        &self,
        ctx: &DecodeContext<'_>,
        root: u32,
        parent: &str,
        reached: &mut BTreeSet<u32>,
        output: &mut Vec<CompoundEntry>,
    ) -> Result<(), CodecError> {
        if root == NO_STREAM {
            return Ok(());
        }
        validate_sibling_tree(&self.directory, root)?;
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            let entry = self.directory.get(id as usize).ok_or_else(|| {
                CodecError::Malformed("CFB directory link is out of range".into())
            })?;
            if !reached.insert(id) {
                return malformed("CFB directory entry belongs to more than one storage");
            }
            ctx.charge_work(1, "traverse CFB directory")?;
            if entry.right != NO_STREAM {
                pending.push(entry.right);
            }
            let path = if parent.is_empty() {
                entry.name.clone()
            } else {
                format!("{parent}/{}", entry.name)
            };
            match entry.object_type {
                1 => {
                    output.push(CompoundEntry::Storage(CompoundStorageEntry {
                        id: CompoundStorageId(CompoundEntryId(id)),
                        path: path.clone(),
                    }));
                    self.walk_tree(ctx, entry.child, &path, reached, output)?;
                }
                2 => {
                    let allocation = if entry.size < self.mini_stream_cutoff {
                        CompoundAllocation::Mini
                    } else {
                        CompoundAllocation::Regular
                    };
                    let sector_size = match allocation {
                        CompoundAllocation::Regular => self.sector_size,
                        CompoundAllocation::Mini => self.mini_sector_size,
                    };
                    let expected = usize::try_from(entry.size)
                        .map_err(|_| {
                            CodecError::Malformed("CFB stream size does not fit memory".into())
                        })?
                        .div_ceil(sector_size);
                    let chain = if entry.size == 0 {
                        if !matches!(entry.start_sector, END_OF_CHAIN | FREE_SECTOR) {
                            return malformed(format!(
                                "empty CFB stream {path} has an invalid start sector"
                            ));
                        }
                        Vec::new()
                    } else {
                        match allocation {
                            CompoundAllocation::Regular => chain(
                                &self.fat,
                                self.sector_count,
                                entry.start_sector,
                                Some(expected),
                                "stream",
                            )?,
                            CompoundAllocation::Mini => chain(
                                &self.mini_fat,
                                self.mini_fat.len(),
                                entry.start_sector,
                                Some(expected),
                                "mini stream",
                            )?,
                        }
                    };
                    output.push(CompoundEntry::Stream(CompoundStreamEntry {
                        id: CompoundStreamId(CompoundEntryId(id)),
                        path,
                        logical_size: entry.size,
                        start_sector: entry.start_sector,
                        allocation,
                        chain,
                    }));
                }
                _ => return malformed("root or empty object appears in a storage child tree"),
            }
            if entry.left != NO_STREAM {
                pending.push(entry.left);
            }
        }
        Ok(())
    }

    fn validate_sector_ownership(&self) -> Result<(), CodecError> {
        let mut used = BTreeSet::new();
        for &sector in self
            .fat_sectors
            .iter()
            .chain(&self.difat_sectors)
            .chain(&self.directory_chain)
            .chain(&self.mini_fat_chain)
            .chain(&self.root_mini_chain)
        {
            if !used.insert(sector) {
                return malformed("CFB regular sector has duplicate structural ownership");
            }
        }
        let mut mini_used = BTreeSet::new();
        let mini_capacity = usize::try_from(self.directory[0].size)
            .map_err(|_| {
                CodecError::Malformed("CFB root mini-stream size does not fit memory".into())
            })?
            .div_ceil(self.mini_sector_size);
        for entry in self.build_entries_unbudgeted()? {
            if let CompoundEntry::Stream(stream) = entry {
                let target = if stream.allocation == CompoundAllocation::Regular {
                    &mut used
                } else {
                    &mut mini_used
                };
                for sector in stream.chain {
                    if stream.allocation == CompoundAllocation::Mini
                        && sector as usize >= mini_capacity
                    {
                        return malformed("CFB mini stream escapes the root mini stream");
                    }
                    if !target.insert(sector) {
                        return malformed("CFB stream sector has duplicate ownership");
                    }
                }
            }
        }
        Ok(())
    }

    fn build_entries_unbudgeted(&self) -> Result<Vec<CompoundEntry>, CodecError> {
        fn walk(
            state: &CompoundState,
            root: u32,
            parent: &str,
            reached: &mut BTreeSet<u32>,
            output: &mut Vec<CompoundEntry>,
        ) -> Result<(), CodecError> {
            if root == NO_STREAM {
                return Ok(());
            }
            let mut pending = vec![root];
            while let Some(id) = pending.pop() {
                let entry = state.directory.get(id as usize).ok_or_else(|| {
                    CodecError::Malformed("CFB directory link is out of range".into())
                })?;
                if !reached.insert(id) {
                    return malformed("CFB directory entry belongs to more than one storage");
                }
                if entry.right != NO_STREAM {
                    pending.push(entry.right);
                }
                let path = if parent.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{parent}/{}", entry.name)
                };
                if entry.object_type == 1 {
                    output.push(CompoundEntry::Storage(CompoundStorageEntry {
                        id: CompoundStorageId(CompoundEntryId(id)),
                        path: path.clone(),
                    }));
                    walk(state, entry.child, &path, reached, output)?;
                } else if entry.object_type == 2 {
                    let allocation = if entry.size < state.mini_stream_cutoff {
                        CompoundAllocation::Mini
                    } else {
                        CompoundAllocation::Regular
                    };
                    let width = if allocation == CompoundAllocation::Mini {
                        state.mini_sector_size
                    } else {
                        state.sector_size
                    };
                    let expected = usize::try_from(entry.size)
                        .map_err(|_| {
                            CodecError::Malformed("CFB stream size does not fit memory".into())
                        })?
                        .div_ceil(width);
                    let chain = if entry.size == 0 {
                        Vec::new()
                    } else if allocation == CompoundAllocation::Mini {
                        chain(
                            &state.mini_fat,
                            state.mini_fat.len(),
                            entry.start_sector,
                            Some(expected),
                            "mini stream",
                        )?
                    } else {
                        chain(
                            &state.fat,
                            state.sector_count,
                            entry.start_sector,
                            Some(expected),
                            "stream",
                        )?
                    };
                    output.push(CompoundEntry::Stream(CompoundStreamEntry {
                        id: CompoundStreamId(CompoundEntryId(id)),
                        path,
                        logical_size: entry.size,
                        start_sector: entry.start_sector,
                        allocation,
                        chain,
                    }));
                }
                if entry.left != NO_STREAM {
                    pending.push(entry.left);
                }
            }
            Ok(())
        }
        let mut reached = BTreeSet::new();
        let mut output = Vec::new();
        walk(self, self.directory[0].child, "", &mut reached, &mut output)?;
        Ok(output)
    }
}

/// Result of a bounded prefix-only CFB probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompoundPrefixProbe {
    /// The prefix does not start with the CFB signature.
    NotCompound,
    /// The prefix ends before a required reachable sector.
    Incomplete,
    /// The available CFB structure is invalid.
    Malformed(String),
    /// Directory paths were reached structurally from the root storage.
    DirectoryEvidence(Vec<String>),
}

impl CompoundPrefixProbe {
    /// Parses only complete structures available in `prefix`.
    pub fn inspect(prefix: &[u8]) -> Self {
        if prefix.get(..8) != Some(&MAGIC) {
            return Self::NotCompound;
        }
        let Some(major) = le_u16(prefix, 26) else {
            return Self::Incomplete;
        };
        let Some(shift) = le_u16(prefix, 30) else {
            return Self::Incomplete;
        };
        let sector_size = match (major, shift) {
            (3, 9) => 512,
            (4, 12) => 4096,
            _ => return Self::Malformed("invalid CFB sector layout".into()),
        };
        if prefix.len() < sector_size {
            return Self::Incomplete;
        }
        if prefix.get(8..24) != Some(&[0; 16])
            || le_u16(prefix, 24) != Some(0x003e)
            || le_u16(prefix, 28) != Some(0xfffe)
        {
            return Self::Malformed("invalid CFB header".into());
        }
        let Some(fat_count) = le_u32(prefix, 44).and_then(|v| usize::try_from(v).ok()) else {
            return Self::Incomplete;
        };
        let Some(directory_start) = le_u32(prefix, 48) else {
            return Self::Incomplete;
        };
        let available = (prefix.len() - sector_size) / sector_size;
        let mut fat_sectors = Vec::new();
        for index in 0..109 {
            let Some(id) = le_u32(prefix, 76 + index * 4) else {
                return Self::Incomplete;
            };
            if id != FREE_SECTOR {
                fat_sectors.push(id);
            }
        }
        if fat_sectors.len() != fat_count {
            return Self::Incomplete;
        }
        let mut fat = Vec::new();
        for id in fat_sectors {
            if id as usize >= available {
                return Self::Incomplete;
            }
            let Some(raw) = sector_slice(prefix, sector_size, available, id) else {
                return Self::Incomplete;
            };
            fat.extend(
                raw.chunks_exact(4)
                    .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte chunk"))),
            );
        }
        let chain = match chain(&fat, available, directory_start, None, "directory") {
            Ok(value) => value,
            Err(error) => return Self::Malformed(error.to_string()),
        };
        if chain.iter().any(|id| *id as usize >= available) {
            return Self::Incomplete;
        }
        let Ok(directory_bytes) = join_sectors(prefix, sector_size, available, &chain) else {
            return Self::Incomplete;
        };
        let directory = match parse_directory(&directory_bytes, major) {
            Ok(value) => value,
            Err(error) => return Self::Malformed(error.to_string()),
        };
        if let Err(error) = validate_root(&directory) {
            return Self::Malformed(error.to_string());
        }
        let mut names = Vec::new();
        let mut pending = vec![(directory[0].child, String::new())];
        let mut seen = BTreeSet::new();
        while let Some((id, parent)) = pending.pop() {
            if id == NO_STREAM {
                continue;
            }
            let Some(entry) = directory.get(id as usize) else {
                return Self::Malformed("CFB directory link is out of range".into());
            };
            if !seen.insert(id) {
                return Self::Malformed("CFB directory link cycle".into());
            }
            let path = if parent.is_empty() {
                entry.name.clone()
            } else {
                format!("{parent}/{}", entry.name)
            };
            names.push(path.clone());
            pending.push((entry.left, parent.clone()));
            pending.push((entry.right, parent.clone()));
            if entry.object_type == 1 {
                pending.push((entry.child, path));
            }
        }
        Self::DirectoryEvidence(names)
    }

    /// Returns structurally reached paths when the prefix provides usable evidence.
    pub fn paths(&self) -> Option<&[String]> {
        match self {
            Self::DirectoryEvidence(paths) => Some(paths),
            _ => None,
        }
    }
}

fn parse_directory(bytes: &[u8], major_version: u16) -> Result<Vec<DirectoryEntry>, CodecError> {
    if !bytes.len().is_multiple_of(128) {
        return malformed("CFB directory stream has a partial entry");
    }
    let mut entries = Vec::with_capacity(bytes.len() / 128);
    for raw in bytes.chunks_exact(128) {
        let object_type = raw[66];
        if !matches!(object_type, 0 | 1 | 2 | 5) {
            return malformed("invalid CFB directory object type");
        }
        let name_len = usize::from(u16::from_le_bytes([raw[64], raw[65]]));
        let name = if object_type == 0 {
            if name_len != 0 {
                return malformed("empty CFB directory entry has a name");
            }
            String::new()
        } else {
            if !(2..=64).contains(&name_len)
                || !name_len.is_multiple_of(2)
                || raw[name_len - 2..name_len] != [0, 0]
            {
                return malformed("invalid CFB directory name length or terminator");
            }
            let units = raw[..name_len - 2]
                .chunks_exact(2)
                .map(|word| u16::from_le_bytes([word[0], word[1]]))
                .collect::<Vec<_>>();
            let name = String::from_utf16(&units)
                .map_err(|_| CodecError::Malformed("invalid UTF-16 CFB directory name".into()))?;
            if name
                .chars()
                .any(|character| matches!(character, '/' | '\\' | ':' | '!'))
            {
                return malformed("CFB directory name contains a forbidden character");
            }
            name
        };
        let color = raw[67];
        if object_type != 0 && color > 1 {
            return malformed("invalid CFB directory node color");
        }
        let mut size = u64::from_le_bytes(raw[120..128].try_into().expect("eight-byte field"));
        if major_version == 3 {
            size &= 0xffff_ffff;
        }
        entries.push(DirectoryEntry {
            name,
            object_type,
            color,
            left: u32::from_le_bytes(raw[68..72].try_into().expect("four-byte field")),
            right: u32::from_le_bytes(raw[72..76].try_into().expect("four-byte field")),
            child: u32::from_le_bytes(raw[76..80].try_into().expect("four-byte field")),
            start_sector: u32::from_le_bytes(raw[116..120].try_into().expect("four-byte field")),
            size,
        });
    }
    Ok(entries)
}

fn validate_root(directory: &[DirectoryEntry]) -> Result<(), CodecError> {
    let root = directory
        .first()
        .ok_or_else(|| CodecError::Malformed("empty CFB directory".into()))?;
    if root.object_type != 5
        || root.name != "Root Entry"
        || root.color != 1
        || root.left != NO_STREAM
        || root.right != NO_STREAM
    {
        return malformed("invalid CFB root directory entry");
    }
    if directory.iter().skip(1).any(|entry| entry.object_type == 5) {
        return malformed("CFB directory has more than one root entry");
    }
    Ok(())
}

fn validate_sibling_tree(directory: &[DirectoryEntry], root: u32) -> Result<(), CodecError> {
    if root == NO_STREAM {
        return Ok(());
    }
    let root_entry = directory
        .get(root as usize)
        .ok_or_else(|| CodecError::Malformed("CFB sibling root is out of range".into()))?;
    if root_entry.color != 1 {
        return malformed("CFB sibling-tree root is not black");
    }
    visit_sibling_tree(directory, root, None, None, false, &mut BTreeSet::new()).map(|_| ())
}

fn visit_sibling_tree(
    directory: &[DirectoryEntry],
    id: u32,
    lower: Option<&str>,
    upper: Option<&str>,
    parent_red: bool,
    seen: &mut BTreeSet<u32>,
) -> Result<usize, CodecError> {
    if id == NO_STREAM {
        return Ok(1);
    }
    let entry = directory
        .get(id as usize)
        .ok_or_else(|| CodecError::Malformed("CFB sibling link is out of range".into()))?;
    if !matches!(entry.object_type, 1 | 2) || !seen.insert(id) {
        return malformed("CFB sibling tree contains an invalid node or cycle");
    }
    if lower.is_some_and(|name| cfb_name_cmp(name, &entry.name) != Ordering::Less)
        || upper.is_some_and(|name| cfb_name_cmp(&entry.name, name) != Ordering::Less)
    {
        return malformed("CFB sibling tree violates directory-name ordering");
    }
    let red = entry.color == 0;
    if red && parent_red {
        return malformed("CFB sibling tree contains adjacent red nodes");
    }
    let left = visit_sibling_tree(directory, entry.left, lower, Some(&entry.name), red, seen)?;
    let right = visit_sibling_tree(directory, entry.right, Some(&entry.name), upper, red, seen)?;
    if left != right {
        return malformed("CFB sibling tree has unequal black height");
    }
    Ok(left + usize::from(!red))
}

fn cfb_name_cmp(left: &str, right: &str) -> Ordering {
    let left_len = left.encode_utf16().count();
    let right_len = right.encode_utf16().count();
    left_len.cmp(&right_len).then_with(|| {
        left.to_uppercase()
            .encode_utf16()
            .cmp(right.to_uppercase().encode_utf16())
    })
}

fn path_key(path: &str) -> String {
    path.to_uppercase()
}

fn chain(
    fat: &[u32],
    sector_count: usize,
    start: u32,
    expected: Option<usize>,
    role: &str,
) -> Result<Vec<u32>, CodecError> {
    if expected == Some(0) {
        return if matches!(start, END_OF_CHAIN | FREE_SECTOR) {
            Ok(Vec::new())
        } else {
            malformed(format!("empty CFB {role} has an invalid start sector"))
        };
    }
    let limit = expected.unwrap_or(sector_count);
    let mut output = Vec::with_capacity(expected.unwrap_or(0));
    let mut seen = BTreeSet::new();
    let mut current = start;
    while current != END_OF_CHAIN {
        if current >= sector_count as u32 || !seen.insert(current) || output.len() >= limit {
            return malformed(format!(
                "CFB {role} chain is cyclic, overlong, or out of range"
            ));
        }
        output.push(current);
        current = *fat
            .get(current as usize)
            .ok_or_else(|| CodecError::Malformed(format!("CFB {role} FAT link is absent")))?;
        if matches!(current, FREE_SECTOR | FAT_SECTOR | DIFAT_SECTOR) {
            return malformed(format!("CFB {role} chain enters a reserved sector role"));
        }
    }
    if expected.is_some_and(|count| output.len() != count) {
        return malformed(format!(
            "CFB {role} chain length does not match its declaration"
        ));
    }
    Ok(output)
}

fn join_sectors(
    bytes: &[u8],
    sector_size: usize,
    sector_count: usize,
    sectors: &[u32],
) -> Result<Vec<u8>, CodecError> {
    let length = sectors
        .len()
        .checked_mul(sector_size)
        .ok_or_else(|| CodecError::Malformed("CFB chain byte length overflow".into()))?;
    let mut output = Vec::with_capacity(length);
    for &sector in sectors {
        output.extend_from_slice(
            sector_slice(bytes, sector_size, sector_count, sector)
                .ok_or_else(|| CodecError::Malformed("CFB sector is absent".into()))?,
        );
    }
    Ok(output)
}

fn physically_contiguous(views: &[View<'_>]) -> bool {
    views
        .windows(2)
        .all(|pair| pair[0].end() == pair[1].start())
}

fn push_span(
    spans: &mut Vec<CompoundPhysicalSpan>,
    start: u64,
    length: usize,
    role: &str,
    entry: Option<String>,
) {
    if length == 0 {
        return;
    }
    spans.push(CompoundPhysicalSpan {
        start,
        end: start + length as u64,
        role: role.into(),
        entry,
    });
}

fn sector_range(sector_size: usize, count: usize, id: u32) -> Result<(usize, usize), CodecError> {
    let index = usize::try_from(id)
        .map_err(|_| CodecError::Malformed("CFB sector id does not fit memory".into()))?;
    if index >= count {
        return malformed("CFB sector id is out of range");
    }
    let start = sector_size
        .checked_add(
            index
                .checked_mul(sector_size)
                .ok_or_else(|| CodecError::Malformed("CFB sector offset overflow".into()))?,
        )
        .ok_or_else(|| CodecError::Malformed("CFB sector offset overflow".into()))?;
    Ok((start, start + sector_size))
}

fn sector_slice(bytes: &[u8], sector_size: usize, count: usize, id: u32) -> Option<&[u8]> {
    let (start, end) = sector_range(sector_size, count, id).ok()?;
    bytes.get(start..end)
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}
fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}
fn malformed<T>(message: impl Into<String>) -> Result<T, CodecError> {
    Err(CodecError::Malformed(message.into()))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::*;

    const SECTOR_SIZE: usize = 512;

    #[test]
    fn snapshot_opens_regular_and_mini_streams_lazily() {
        let file = fixture();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic CFB parses");
        assert_eq!(snapshot.major_version(), 3);
        assert_eq!(
            snapshot
                .open(&ctx, snapshot.stream("small").expect("small stream exists"),)
                .expect("small stream opens")
                .window(),
            b"small"
        );
        assert_eq!(
            snapshot
                .open(
                    &ctx,
                    snapshot
                        .stream("Store/Large")
                        .expect("regular stream exists"),
                )
                .expect("regular stream opens")
                .window(),
            vec![0x5a; 4096]
        );
        assert!(matches!(
            snapshot.entry("STORE"),
            Some(CompoundEntry::Storage(_))
        ));
        assert_eq!(
            snapshot
                .physical_ledger()
                .expect("physical ledger builds")
                .last()
                .expect("ledger contains the header")
                .end,
            file.len() as u64
        );
    }

    #[test]
    fn prefix_probe_reaches_directory_names_without_scanning_bytes() {
        let file = fixture();
        let CompoundPrefixProbe::DirectoryEvidence(paths) = CompoundPrefixProbe::inspect(&file)
        else {
            panic!("usable prefix")
        };
        assert!(paths.iter().any(|path| path == "Store/Large"));
        assert_eq!(
            CompoundPrefixProbe::inspect(b"not cfb"),
            CompoundPrefixProbe::NotCompound
        );
        assert_eq!(
            CompoundPrefixProbe::inspect(&file[..400]),
            CompoundPrefixProbe::Incomplete
        );
    }

    #[test]
    fn rejects_duplicate_allocation_and_cyclic_chains() {
        let mut file = fixture();
        put_u32(sector_mut(&mut file, 11), 2 * 4, 2);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        assert!(CompoundSnapshot::new(&ctx, root).is_err());
    }

    pub(crate) fn fixture() -> Vec<u8> {
        let mut file = vec![0u8; SECTOR_SIZE * 13];
        file[..8].copy_from_slice(&MAGIC);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 3);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 9);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 44, 1);
        put_u32(&mut file, 48, 0);
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, 10);
        put_u32(&mut file, 64, 1);
        put_u32(&mut file, 68, END_OF_CHAIN);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, FREE_SECTOR);
        }
        put_u32(&mut file, 76, 11);
        let directory = sector_mut(&mut file, 0);
        directory_entry(
            directory,
            0,
            "Root Entry",
            5,
            NO_STREAM,
            NO_STREAM,
            1,
            1,
            512,
        );
        directory_entry(directory, 1, "Small", 2, NO_STREAM, 2, NO_STREAM, 0, 5);
        directory_entry(directory, 2, "Store", 1, NO_STREAM, NO_STREAM, 3, 0, 0);
        directory[2 * 128 + 67] = 0;
        directory_entry(
            directory, 3, "Large", 2, NO_STREAM, NO_STREAM, NO_STREAM, 2, 4096,
        );
        sector_mut(&mut file, 1)[..5].copy_from_slice(b"small");
        for id in 2..=9 {
            sector_mut(&mut file, id).fill(0x5a);
        }
        let mini_fat = sector_mut(&mut file, 10);
        mini_fat.fill(0xff);
        put_u32(mini_fat, 0, END_OF_CHAIN);
        let fat = sector_mut(&mut file, 11);
        fat.fill(0xff);
        put_u32(fat, 0, END_OF_CHAIN);
        put_u32(fat, 4, END_OF_CHAIN);
        for id in 2..9 {
            put_u32(fat, id * 4, (id + 1) as u32);
        }
        put_u32(fat, 9 * 4, END_OF_CHAIN);
        put_u32(fat, 10 * 4, END_OF_CHAIN);
        put_u32(fat, 11 * 4, FAT_SECTOR);
        file
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "field-level synthetic CFB directory builder"
    )]
    fn directory_entry(
        directory: &mut [u8],
        index: usize,
        name: &str,
        object_type: u8,
        left: u32,
        right: u32,
        child: u32,
        start_sector: u32,
        size: u64,
    ) {
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let encoded = name.encode_utf16().collect::<Vec<_>>();
        for (offset, unit) in encoded.iter().enumerate() {
            put_u16(entry, offset * 2, *unit);
        }
        put_u16(entry, 64, ((encoded.len() + 1) * 2) as u16);
        entry[66] = object_type;
        entry[67] = 1;
        put_u32(entry, 68, left);
        put_u32(entry, 72, right);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start_sector);
        entry[120..128].copy_from_slice(&size.to_le_bytes());
    }

    fn sector_mut(file: &mut [u8], id: usize) -> &mut [u8] {
        let start = SECTOR_SIZE * (id + 1);
        &mut file[start..start + SECTOR_SIZE]
    }
    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
