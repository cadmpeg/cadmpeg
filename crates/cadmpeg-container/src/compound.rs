// SPDX-License-Identifier: Apache-2.0
//! Lazy, budgeted Microsoft Compound File Binary (CFB) snapshots.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use cadmpeg_core::decode::{ByteRange, DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};

use crate::SpanRole;

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const DIFAT_SECTOR: u32 = 0xffff_fffc;
const NO_STREAM: u32 = 0xffff_ffff;
const V3_MAX_FILE_SIZE: u64 = 0x8000_0000;
const RANGE_LOCK_START: u64 = 0x7fff_ff00;
const RANGE_LOCK_END: u64 = 0x8000_0000;

static NEXT_COMPOUND_SNAPSHOT_ID: AtomicU64 = AtomicU64::new(1);

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
    snapshot_id: u64,
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
    /// CFB structural role, including an owning entry where applicable.
    pub role: SpanRole,
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
    range_lock_sector: Option<u32>,
}

/// Parsed CFB navigation state over one decode-session root view.
#[derive(Debug)]
pub struct CompoundSnapshot<'a> {
    root: View<'a>,
    snapshot_id: u64,
    parsed: CompoundState,
    entries: Vec<CompoundEntry>,
    by_path: BTreeMap<Vec<Vec<u16>>, usize>,
    streams_by_id: BTreeMap<CompoundStreamId, usize>,
}

impl<'a> CompoundSnapshot<'a> {
    /// Parses and validates the complete CFB structure without opening streams.
    /// Stream extents are checked against their available bytes when opened.
    pub fn new(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let parsed = CompoundState::parse(ctx, root.window())?;
        let snapshot_id = NEXT_COMPOUND_SNAPSHOT_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let mut entries = parsed.build_entries(ctx)?;
        for entry in &mut entries {
            if let CompoundEntry::Stream(stream) = entry {
                stream.snapshot_id = snapshot_id;
            }
        }
        parsed.validate_sector_ownership(&entries)?;
        let mut by_path = BTreeMap::new();
        let mut streams_by_id = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let component_count = entry.path().split('/').count();
            let unit_count = entry.path().encode_utf16().count();
            let key_bytes = component_count
                .checked_mul(std::mem::size_of::<Vec<u16>>())
                .and_then(|bytes| {
                    unit_count
                        .checked_mul(std::mem::size_of::<u16>())
                        .and_then(|units| bytes.checked_add(units))
                })
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<(Vec<Vec<u16>>, usize)>()))
                .ok_or_else(|| CodecError::Malformed("CFB path index size overflow".into()))?;
            ctx.charge_retained(
                key_bytes as u64,
                "retain CFB path index",
                Some(root.location()),
            )?;
            ctx.charge_collection_items(1, "index CFB path")?;
            let key = path_key(entry.path());
            if by_path.insert(key, index).is_some() {
                return malformed(format!("duplicate CFB path {}", entry.path()));
            }
            if let CompoundEntry::Stream(stream) = entry {
                ctx.charge_retained(
                    std::mem::size_of::<(CompoundStreamId, usize)>() as u64,
                    "retain CFB stream index",
                    Some(root.location()),
                )?;
                ctx.charge_collection_items(1, "index CFB stream")?;
                streams_by_id.insert(stream.id(), index);
            }
        }
        Ok(Self {
            root,
            snapshot_id,
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
        if entry.snapshot_id != self.snapshot_id || self.stream_by_id(entry.id()).is_none() {
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
                CodecError::malformed(format_args!("empty allocation chain for {}", entry.path))
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
                CodecError::malformed(format_args!(
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
            structural.insert(sector, SpanRole::CfbFat);
        }
        for &sector in &self.parsed.difat_sectors {
            structural.insert(sector, SpanRole::CfbDifat);
        }
        for &sector in &self.parsed.directory_chain {
            structural.insert(sector, SpanRole::CfbDirectory);
        }
        for &sector in &self.parsed.mini_fat_chain {
            structural.insert(sector, SpanRole::CfbMiniFat);
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
            role: SpanRole::CfbHeader,
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
            let sector_end = start
                .checked_add(self.parsed.sector_size as u64)
                .ok_or_else(|| CodecError::Malformed("CFB ledger offset overflow".into()))?
                .min(self.root.window().len() as u64);
            let sector_length = usize::try_from(sector_end.saturating_sub(start))
                .map_err(|_| CodecError::Malformed("CFB ledger sector length overflow".into()))?;
            let sector = u32::try_from(index)
                .map_err(|_| CodecError::Malformed("CFB sector id exceeds u32".into()))?;
            if self.parsed.range_lock_sector == Some(sector) {
                push_span(
                    &mut spans,
                    start,
                    sector_length,
                    SpanRole::CfbRangeLockSector,
                );
            } else if let Some(role) = structural.get(&sector) {
                push_span(&mut spans, start, sector_length, role.clone());
            } else if let Some((entry, payload)) = regular.get(&sector) {
                if *payload > sector_length {
                    return malformed(format!("CFB stream {entry} is shorter than declared"));
                }
                push_span(
                    &mut spans,
                    start,
                    *payload,
                    SpanRole::CfbRegularStreamPayload(entry.clone()),
                );
                push_span(
                    &mut spans,
                    start + *payload as u64,
                    sector_length - *payload,
                    SpanRole::CfbEntryPadding(entry.clone()),
                );
            } else if let Some(root_ordinal) = root_sectors.get(&sector) {
                for mini_ordinal in 0..sector_length.div_ceil(self.parsed.mini_sector_size) {
                    let logical_mini = root_ordinal
                        .checked_mul(self.parsed.sector_size / self.parsed.mini_sector_size)
                        .and_then(|base| base.checked_add(mini_ordinal))
                        .ok_or_else(|| {
                            CodecError::Malformed("CFB mini-sector id overflow".into())
                        })?;
                    let mini_offset = mini_ordinal * self.parsed.mini_sector_size;
                    let mini_length =
                        (sector_length - mini_offset).min(self.parsed.mini_sector_size);
                    let mini_start = start + mini_offset as u64;
                    let root_offset = (logical_mini * self.parsed.mini_sector_size) as u64;
                    let mapped = root_size
                        .saturating_sub(root_offset)
                        .min(mini_length as u64) as usize;
                    let logical_mini = u32::try_from(logical_mini).map_err(|_| {
                        CodecError::Malformed("CFB mini-sector id exceeds u32".into())
                    })?;
                    if let Some((entry, payload)) = mini.get(&logical_mini) {
                        if *payload > mini_length {
                            return malformed(format!(
                                "CFB stream {entry} is shorter than declared"
                            ));
                        }
                        push_span(
                            &mut spans,
                            mini_start,
                            *payload,
                            SpanRole::CfbMiniStreamPayload(entry.clone()),
                        );
                        push_span(
                            &mut spans,
                            mini_start + *payload as u64,
                            mini_length - *payload,
                            SpanRole::CfbEntryPadding(entry.clone()),
                        );
                    } else {
                        push_span(
                            &mut spans,
                            mini_start,
                            mapped,
                            SpanRole::CfbMiniStreamPadding,
                        );
                        push_span(
                            &mut spans,
                            mini_start + mapped as u64,
                            mini_length - mapped,
                            SpanRole::CfbPadding,
                        );
                    }
                }
            } else {
                push_span(
                    &mut spans,
                    start,
                    sector_length,
                    SpanRole::CfbUnallocatedSector,
                );
            }
        }
        if spans.first().is_none_or(|span| span.start != 0)
            || spans.windows(2).any(|pair| pair[0].end != pair[1].start)
            || spans
                .last()
                .is_none_or(|span| span.end != self.root.window().len() as u64)
        {
            return malformed("physical CFB ledger has a gap or overlap");
        }
        Ok(spans)
    }

    fn regular_sector_view(&self, sector: u32) -> Result<View<'a>, CodecError> {
        let (start, end) = sector_range(
            self.parsed.sector_size,
            self.parsed.sector_count,
            self.root.window().len(),
            sector,
        )?;
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
                .ok_or_else(|| CodecError::malformed(format_args!("truncated CFB {what}")))
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
        if bytes.len() < sector_size {
            return malformed("CFB input does not contain a complete header sector");
        }
        let sector_count = (bytes.len() - sector_size).div_ceil(sector_size);
        if sector_count < 2 {
            return malformed("CFB file has fewer than the minimum three sectors");
        }
        if major_version == 3 && bytes.len() as u64 > V3_MAX_FILE_SIZE {
            return malformed("CFB v3 file exceeds the 2 GiB size ceiling");
        }
        if major_version == 4 && bytes[512..sector_size].iter().any(|byte| *byte != 0) {
            return malformed("CFB v4 header padding is not zero");
        }
        let directory_sector_count = usize::try_from(field(40, "directory sector count")?)
            .map_err(|_| {
                CodecError::Malformed("CFB directory sector count does not fit memory".into())
            })?;
        let fat_count = usize::try_from(field(44, "FAT count")?)
            .map_err(|_| CodecError::Malformed("CFB FAT count does not fit memory".into()))?;
        let directory_start = field(48, "directory start")?;
        let _transaction_signature = field(52, "transaction signature")?;
        let mini_stream_cutoff = u64::from(field(56, "mini-stream cutoff")?);
        let mini_fat_start = field(60, "mini FAT start")?;
        let mini_fat_count = usize::try_from(field(64, "mini FAT count")?)
            .map_err(|_| CodecError::Malformed("CFB mini FAT count does not fit memory".into()))?;
        let difat_start = field(68, "DIFAT start")?;
        let difat_count = usize::try_from(field(72, "DIFAT count")?)
            .map_err(|_| CodecError::Malformed("CFB DIFAT count does not fit memory".into()))?;
        if (major_version == 3 && directory_sector_count != 0)
            || (major_version == 4 && directory_sector_count == 0)
            || mini_stream_cutoff != 4096
            || fat_count == 0
            || fat_count > sector_count
            || difat_count > sector_count
        {
            return malformed("invalid CFB header counts or reserved fields");
        }
        ctx.charge_collection_items(
            (fat_count + difat_count) as u64,
            "parse CFB allocation tables",
        )?;
        let allocation_id_bytes = fat_count
            .checked_add(difat_count)
            .and_then(|count| count.checked_mul(std::mem::size_of::<u32>()))
            .ok_or_else(|| CodecError::Malformed("CFB allocation id size overflow".into()))?;
        let allocation_id_scratch = ctx.reserve_scoped(
            allocation_id_bytes as u64,
            "collect CFB allocation sector ids",
            None,
        )?;
        ctx.charge_retained(
            allocation_id_bytes as u64,
            "retain CFB allocation sector ids",
            None,
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
        drop(allocation_id_scratch);
        let fat_word_count = fat_count
            .checked_mul(sector_size / 4)
            .ok_or_else(|| CodecError::Malformed("CFB FAT word count overflow".into()))?;
        ctx.charge_collection_items(fat_word_count as u64, "parse CFB FAT words")?;
        ctx.charge_retained(
            fat_word_count
                .checked_mul(std::mem::size_of::<u32>())
                .ok_or_else(|| CodecError::Malformed("CFB FAT byte size overflow".into()))?
                as u64,
            "retain CFB FAT",
            None,
        )?;
        let mut fat = Vec::with_capacity(fat_word_count);
        for &id in &fat_sectors {
            let data = sector(id)
                .ok_or_else(|| CodecError::Malformed("CFB FAT sector is absent".into()))?;
            if data.len() != sector_size {
                return malformed("CFB FAT sector is truncated");
            }
            fat.extend(
                data.chunks_exact(4)
                    .map(|word| le_u32(word, 0).expect("four-byte chunk")),
            );
        }
        if fat.len() < sector_count {
            return malformed("CFB FAT does not address every physical sector");
        }
        if fat
            .iter()
            .skip(sector_count)
            .any(|entry| *entry != FREE_SECTOR)
        {
            return malformed("CFB FAT entries past end-of-file are not free");
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
        let range_lock_sector = range_lock_sector(major_version, sector_size, bytes.len() as u64)?;
        if range_lock_sector.is_some_and(|id| fat.get(id as usize) != Some(&END_OF_CHAIN)) {
            return malformed("CFB range lock sector is not allocated as an end-of-chain sector");
        }
        let directory_expected = (major_version == 4).then_some(directory_sector_count);
        let directory_chain = chain(
            Some(ctx),
            &fat,
            sector_count,
            directory_start,
            directory_expected,
            "directory",
        )?;
        let directory_byte_count = directory_chain
            .len()
            .checked_mul(sector_size)
            .ok_or_else(|| CodecError::Malformed("CFB directory byte size overflow".into()))?;
        let directory_scratch = ctx.reserve_scoped(
            directory_byte_count as u64,
            "assemble CFB directory sectors",
            None,
        )?;
        let directory_bytes = join_sectors(bytes, sector_size, sector_count, &directory_chain)?;
        let directory = parse_directory(Some(ctx), &directory_bytes, major_version)?;
        drop(directory_scratch);
        validate_root(&directory)?;
        let mini_fat_chain = if mini_fat_count == 0 {
            if mini_fat_start != END_OF_CHAIN {
                return malformed("empty CFB mini FAT has a start sector");
            }
            Vec::new()
        } else {
            chain(
                Some(ctx),
                &fat,
                sector_count,
                mini_fat_start,
                Some(mini_fat_count),
                "mini FAT",
            )?
        };
        let mini_fat_byte_count = mini_fat_chain
            .len()
            .checked_mul(sector_size)
            .ok_or_else(|| CodecError::Malformed("CFB mini FAT byte size overflow".into()))?;
        let mini_fat_scratch = ctx.reserve_scoped(
            mini_fat_byte_count as u64,
            "assemble CFB mini FAT sectors",
            None,
        )?;
        let mini_fat_word_count = mini_fat_byte_count / 4;
        ctx.charge_collection_items(mini_fat_word_count as u64, "parse CFB mini FAT words")?;
        ctx.charge_retained(mini_fat_byte_count as u64, "retain CFB mini FAT", None)?;
        let mini_fat = join_sectors(bytes, sector_size, sector_count, &mini_fat_chain)?
            .chunks_exact(4)
            .map(|word| le_u32(word, 0).expect("four-byte chunk"))
            .collect::<Vec<_>>();
        drop(mini_fat_scratch);
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
                Some(ctx),
                &fat,
                sector_count,
                root.start_sector,
                Some(root_sectors),
                "root mini stream",
            )?
        };
        Ok(Self {
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
            range_lock_sector,
        })
    }

    fn build_entries(&self, ctx: &DecodeContext<'_>) -> Result<Vec<CompoundEntry>, CodecError> {
        let scratch_bytes = self
            .directory
            .len()
            .checked_mul(
                std::mem::size_of::<u32>().saturating_add(std::mem::size_of::<(u32, String)>()),
            )
            .ok_or_else(|| CodecError::Malformed("CFB traversal scratch size overflow".into()))?;
        let _scratch = ctx.reserve_scoped(scratch_bytes as u64, "traverse CFB directory", None)?;
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
                ctx.charge_retained(
                    entry
                        .name
                        .len()
                        .saturating_add(std::mem::size_of::<CompoundEntry>())
                        as u64,
                    "retain CFB entry",
                    None,
                )?;
                entry.name.clone()
            } else {
                let path_len = parent
                    .len()
                    .checked_add(1)
                    .and_then(|length| length.checked_add(entry.name.len()))
                    .ok_or_else(|| CodecError::Malformed("CFB path length overflow".into()))?;
                ctx.charge_retained(
                    path_len
                        .checked_add(std::mem::size_of::<CompoundEntry>())
                        .ok_or_else(|| {
                            CodecError::Malformed("CFB entry storage size overflow".into())
                        })? as u64,
                    "retain CFB entry",
                    None,
                )?;
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
                                Some(ctx),
                                &self.fat,
                                self.sector_count,
                                entry.start_sector,
                                Some(expected),
                                "stream",
                            )?,
                            CompoundAllocation::Mini => chain(
                                Some(ctx),
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
                        snapshot_id: 0,
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

    fn validate_sector_ownership(&self, entries: &[CompoundEntry]) -> Result<(), CodecError> {
        let mut used = BTreeSet::new();
        if let Some(sector) = self.range_lock_sector {
            used.insert(sector);
        }
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
        for entry in entries {
            if let CompoundEntry::Stream(stream) = entry {
                let target = if stream.allocation == CompoundAllocation::Regular {
                    &mut used
                } else {
                    &mut mini_used
                };
                let mut remaining = stream.logical_size;
                for &sector in &stream.chain {
                    let payload = remaining.min(self.mini_sector_size as u64);
                    remaining = remaining.saturating_sub(payload);
                    if stream.allocation == CompoundAllocation::Mini
                        && (sector as usize >= mini_capacity
                            || u64::from(sector)
                                .saturating_mul(self.mini_sector_size as u64)
                                .saturating_add(payload)
                                > self.directory[0].size)
                    {
                        return malformed("CFB mini stream escapes the root mini stream");
                    }
                    if !target.insert(sector) {
                        return malformed("CFB stream sector has duplicate ownership");
                    }
                }
            }
        }
        for (sector, marker) in self.mini_fat.iter().enumerate() {
            let sector = u32::try_from(sector)
                .map_err(|_| CodecError::Malformed("CFB mini-sector id exceeds u32".into()))?;
            if !mini_used.contains(&sector) && *marker != FREE_SECTOR {
                return malformed("unowned CFB mini sector is not marked free");
            }
        }
        for (sector, marker) in self.fat.iter().take(self.sector_count).enumerate() {
            let sector = u32::try_from(sector)
                .map_err(|_| CodecError::Malformed("CFB sector id exceeds u32".into()))?;
            if !used.contains(&sector) && *marker != FREE_SECTOR {
                return malformed("unowned CFB sector is not marked free");
            }
        }
        Ok(())
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
            || le_u16(prefix, 32) != Some(6)
            || prefix.get(34..40) != Some(&[0; 6])
            || le_u32(prefix, 56) != Some(4096)
        {
            return Self::Malformed("invalid CFB header".into());
        }
        if major == 4 && prefix[512..sector_size].iter().any(|byte| *byte != 0) {
            return Self::Malformed("CFB v4 header padding is not zero".into());
        }
        let Some(fat_count) = le_u32(prefix, 44).and_then(|v| usize::try_from(v).ok()) else {
            return Self::Incomplete;
        };
        let Some(directory_start) = le_u32(prefix, 48) else {
            return Self::Incomplete;
        };
        let Some(directory_sector_count) = le_u32(prefix, 40) else {
            return Self::Incomplete;
        };
        let Some(difat_start) = le_u32(prefix, 68) else {
            return Self::Incomplete;
        };
        let Some(difat_count) = le_u32(prefix, 72).and_then(|v| usize::try_from(v).ok()) else {
            return Self::Incomplete;
        };
        if fat_count == 0
            || (major == 3 && directory_sector_count != 0)
            || (major == 4 && directory_sector_count == 0)
        {
            return Self::Malformed("invalid CFB header counts".into());
        }
        let available = (prefix.len() - sector_size) / sector_size;
        let mut fat_sectors = Vec::new();
        let mut header_free_seen = false;
        for index in 0..109 {
            let Some(id) = le_u32(prefix, 76 + index * 4) else {
                return Self::Incomplete;
            };
            if id == FREE_SECTOR {
                header_free_seen = true;
            } else {
                if header_free_seen {
                    return Self::Malformed(
                        "non-free CFB header DIFAT entry follows a free entry".into(),
                    );
                }
                fat_sectors.push(id);
            }
        }
        let mut next_difat = difat_start;
        let difat_entries = sector_size / 4 - 1;
        let mut seen_difat = BTreeSet::new();
        for _ in 0..difat_count {
            if next_difat as usize >= available {
                return Self::Incomplete;
            }
            if !seen_difat.insert(next_difat) {
                return Self::Malformed("CFB DIFAT chain is cyclic".into());
            }
            let Some(raw) = sector_slice(prefix, sector_size, available, next_difat) else {
                return Self::Incomplete;
            };
            let mut free_seen = false;
            for index in 0..difat_entries {
                let Some(id) = le_u32(raw, index * 4) else {
                    return Self::Incomplete;
                };
                if id == FREE_SECTOR {
                    free_seen = true;
                } else {
                    if free_seen {
                        return Self::Malformed(
                            "non-free CFB DIFAT entry follows a free entry".into(),
                        );
                    }
                    fat_sectors.push(id);
                }
            }
            let Some(next) = le_u32(raw, difat_entries * 4) else {
                return Self::Incomplete;
            };
            next_difat = next;
        }
        if (difat_count == 0 && difat_start != END_OF_CHAIN)
            || (difat_count != 0 && next_difat != END_OF_CHAIN)
        {
            return Self::Malformed("CFB DIFAT chain length does not match the header".into());
        }
        if fat_sectors.len() != fat_count {
            return Self::Malformed("CFB DIFAT does not match its declared FAT count".into());
        }
        let mut fat = Vec::new();
        let mut loaded_fat_count = 0;
        for &id in &fat_sectors {
            if id as usize >= available {
                break;
            }
            let Some(raw) = sector_slice(prefix, sector_size, available, id) else {
                return Self::Incomplete;
            };
            fat.extend(
                raw.chunks_exact(4)
                    .map(|word| le_u32(word, 0).expect("four-byte chunk")),
            );
            loaded_fat_count += 1;
        }
        if fat_sectors
            .iter()
            .take(loaded_fat_count)
            .any(|id| fat.get(*id as usize) != Some(&FAT_SECTOR))
            || seen_difat.iter().any(|id| {
                fat.get(*id as usize)
                    .is_some_and(|role| role != &DIFAT_SECTOR)
            })
        {
            return Self::Malformed("CFB allocation sector has the wrong role marker".into());
        }
        let expected_directory_count = if major == 4 {
            Some(directory_sector_count as usize)
        } else if directory_sector_count == 0 {
            None
        } else {
            return Self::Malformed("CFB v3 declares directory sector count".into());
        };
        let mut directory_chain = Vec::new();
        let mut seen_directory = BTreeSet::new();
        let mut current = directory_start;
        loop {
            if current as usize >= available {
                return Self::Incomplete;
            }
            let Some(&next) = fat.get(current as usize) else {
                return if loaded_fat_count < fat_count {
                    Self::Incomplete
                } else {
                    Self::Malformed("CFB FAT does not address the directory sector".into())
                };
            };
            if !seen_directory.insert(current) {
                return Self::Malformed("CFB directory chain is cyclic".into());
            }
            directory_chain.push(current);
            if next == END_OF_CHAIN {
                break;
            }
            if next >= DIFAT_SECTOR {
                return Self::Malformed("CFB directory chain has an invalid terminator".into());
            }
            current = next;
        }
        if expected_directory_count.is_some_and(|count| count != directory_chain.len()) {
            return Self::Malformed("CFB directory chain length does not match the header".into());
        }
        let Ok(directory_bytes) = join_sectors(prefix, sector_size, available, &directory_chain)
        else {
            return Self::Incomplete;
        };
        let directory = match parse_directory(None, &directory_bytes, major) {
            Ok(value) => value,
            Err(error) => return Self::Malformed(error.to_string()),
        };
        if let Err(error) = validate_root(&directory) {
            return Self::Malformed(error.to_string());
        }
        if let Err(error) = validate_sibling_tree(&directory, directory[0].child) {
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
                if let Err(error) = validate_sibling_tree(&directory, entry.child) {
                    return Self::Malformed(error.to_string());
                }
                pending.push((entry.child, path));
            }
        }
        if directory
            .iter()
            .enumerate()
            .skip(1)
            .any(|(id, entry)| entry.object_type != 0 && !seen.contains(&(id as u32)))
        {
            return Self::Malformed("CFB directory contains an unreachable live entry".into());
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

/// Reads the prefix used for native-format detection.
///
/// The first read stops at the smaller of `prefix_len` and `max_bytes`.
/// CFB inputs continue until the directory probe settles or `max_bytes` is
/// reached. `FileTooLarge` reports a CFB input that needs bytes beyond that
/// limit to settle the directory probe.
pub fn read_detection_prefix(
    source: &mut dyn Read,
    prefix_len: usize,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    let phase_one_len = prefix_len.min(usize::try_from(max_bytes).unwrap_or(usize::MAX));
    let mut bytes = Vec::with_capacity(phase_one_len);
    let mut chunk =
        cadmpeg_core::decode::alloc_filled(64 * 1024, 0_u8, "compound detection prefix chunk")
            .map_err(io::Error::other)?
            .into_boxed_slice();
    while bytes.len() < phase_one_len {
        let chunk_len = (phase_one_len - bytes.len()).min(chunk.len());
        let read = source.read(&mut chunk[..chunk_len])?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    if matches!(
        CompoundPrefixProbe::inspect(&bytes),
        CompoundPrefixProbe::NotCompound
    ) {
        return Ok(bytes);
    }
    loop {
        if !matches!(
            CompoundPrefixProbe::inspect(&bytes),
            CompoundPrefixProbe::Incomplete
        ) {
            return Ok(bytes);
        }
        if bytes.len() as u64 >= max_bytes {
            if source.read(&mut [0_u8; 1])? != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    "compound detection input exceeds its byte limit",
                ));
            }
            return Ok(bytes);
        }
        let remaining = max_bytes - bytes.len() as u64;
        let chunk_len = remaining.min(chunk.len() as u64) as usize;
        let read = source.read(&mut chunk[..chunk_len])?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn parse_directory(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    major_version: u16,
) -> Result<Vec<DirectoryEntry>, CodecError> {
    if !bytes.len().is_multiple_of(128) {
        return malformed("CFB directory stream has a partial entry");
    }
    let entry_count = bytes.len() / 128;
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(entry_count as u64, "parse CFB directory entries")?;
        let retained = entry_count
            .checked_mul(std::mem::size_of::<DirectoryEntry>())
            .and_then(|size| size.checked_add(bytes.len()))
            .ok_or_else(|| CodecError::Malformed("CFB directory storage size overflow".into()))?;
        ctx.charge_retained(retained as u64, "retain CFB directory", None)?;
    }
    let mut entries = Vec::with_capacity(entry_count);
    for raw in bytes.chunks_exact(128) {
        let object_type = raw[66];
        if object_type == 0 {
            // An unallocated slot has no directory identity. Several writers
            // leave stale bytes in such slots; preserve its index for links,
            // but do not interpret any of those bytes as a live entry.
            entries.push(DirectoryEntry {
                name: String::new(),
                object_type: 0,
                color: 0,
                left: NO_STREAM,
                right: NO_STREAM,
                child: NO_STREAM,
                start_sector: FREE_SECTOR,
                size: 0,
            });
            continue;
        }
        if !matches!(object_type, 1 | 2 | 5) {
            return malformed("invalid CFB directory object type");
        }
        let name_len = usize::from(le_u16(raw, 64).expect("directory name length"));
        let name = {
            if !(2..=64).contains(&name_len)
                || !name_len.is_multiple_of(2)
                || raw[name_len - 2..name_len] != [0, 0]
            {
                return malformed("invalid CFB directory name length or terminator");
            }
            let (name, _) = View::utf16le_at(raw, 0, (name_len - 2) / 2)
                .ok_or_else(|| CodecError::Malformed("invalid UTF-16 CFB directory name".into()))?;
            if name
                .chars()
                .any(|character| matches!(character, '/' | '\\' | ':' | '!'))
            {
                return malformed("CFB directory name contains a forbidden character");
            }
            name
        };
        let color = raw[67];
        if color > 1 {
            return malformed("invalid CFB directory node color");
        }
        let mut size = le_u64(raw, 120).expect("directory stream size");
        if major_version == 3 {
            size &= 0xffff_ffff;
        }
        entries.push(DirectoryEntry {
            name,
            object_type,
            color,
            left: le_u32(raw, 68).expect("directory left pointer"),
            right: le_u32(raw, 72).expect("directory right pointer"),
            child: le_u32(raw, 76).expect("directory child pointer"),
            start_sector: le_u32(raw, 116).expect("directory start sector"),
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
    visit_sibling_tree(directory, root, None, None, false, &mut BTreeSet::new())
}

fn visit_sibling_tree(
    directory: &[DirectoryEntry],
    id: u32,
    lower: Option<&str>,
    upper: Option<&str>,
    parent_red: bool,
    seen: &mut BTreeSet<u32>,
) -> Result<(), CodecError> {
    if id == NO_STREAM {
        return Ok(());
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
    visit_sibling_tree(directory, entry.left, lower, Some(&entry.name), red, seen)?;
    visit_sibling_tree(directory, entry.right, Some(&entry.name), upper, red, seen)
}

fn cfb_name_cmp(left: &str, right: &str) -> Ordering {
    let left_len = left.encode_utf16().count();
    let right_len = right.encode_utf16().count();
    left_len.cmp(&right_len).then_with(|| {
        left.encode_utf16()
            .map(cfb_upper_unit)
            .cmp(right.encode_utf16().map(cfb_upper_unit))
    })
}

fn path_key(path: &str) -> Vec<Vec<u16>> {
    path.split('/')
        .map(|component| component.encode_utf16().map(cfb_upper_unit).collect())
        .collect()
}

fn cfb_upper_unit(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut uppercase = character.to_uppercase();
    let first = uppercase.next().expect("uppercase mapping is non-empty");
    if uppercase.next().is_none() && first.len_utf16() == 1 {
        first as u16
    } else {
        unit
    }
}

fn range_lock_sector(
    major_version: u16,
    sector_size: usize,
    file_size: u64,
) -> Result<Option<u32>, CodecError> {
    if major_version != 4 || file_size <= RANGE_LOCK_END {
        return Ok(None);
    }
    let sector_size = u64::try_from(sector_size)
        .map_err(|_| CodecError::Malformed("CFB sector size does not fit u64".into()))?;
    let physical_sector = RANGE_LOCK_START / sector_size;
    let sector = physical_sector
        .checked_sub(1)
        .ok_or_else(|| CodecError::Malformed("CFB range lock sector underflow".into()))?;
    u32::try_from(sector)
        .map(Some)
        .map_err(|_| CodecError::Malformed("CFB range lock sector exceeds u32".into()))
}

fn chain(
    ctx: Option<&DecodeContext<'_>>,
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
    if let (Some(ctx), Some(count)) = (ctx, expected) {
        ctx.charge_collection_items(count as u64, "retain CFB sector chain")?;
        ctx.charge_retained(
            count
                .checked_mul(std::mem::size_of::<u32>())
                .ok_or_else(|| CodecError::Malformed("CFB sector chain size overflow".into()))?
                as u64,
            "retain CFB sector chain",
            None,
        )?;
    }
    let mut traversal_scratch = ctx
        .map(|ctx| ctx.reserve_scoped(0, "walk CFB sector chain", None))
        .transpose()?;
    let mut output = Vec::with_capacity(expected.unwrap_or(0));
    let mut seen = BTreeSet::new();
    let mut current = start;
    while current != END_OF_CHAIN {
        if current >= sector_count as u32 || !seen.insert(current) || output.len() >= limit {
            return malformed(format!(
                "CFB {role} chain is cyclic, overlong, or out of range"
            ));
        }
        if expected.is_none() {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "retain CFB sector chain")?;
                ctx.charge_retained(
                    std::mem::size_of::<u32>() as u64,
                    "retain CFB sector chain",
                    None,
                )?;
            }
        }
        if let Some(scratch) = &mut traversal_scratch {
            scratch.grow(std::mem::size_of::<u32>() as u64)?;
        }
        output.push(current);
        current = *fat
            .get(current as usize)
            .ok_or_else(|| CodecError::malformed(format_args!("CFB {role} FAT link is absent")))?;
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
        let data = sector_slice(bytes, sector_size, sector_count, sector)
            .ok_or_else(|| CodecError::Malformed("CFB sector is absent".into()))?;
        if data.len() != sector_size {
            return malformed("CFB structural sector is truncated");
        }
        output.extend_from_slice(data);
    }
    Ok(output)
}

fn physically_contiguous(views: &[View<'_>]) -> bool {
    views
        .windows(2)
        .all(|pair| pair[0].end() == pair[1].start())
}

fn push_span(spans: &mut Vec<CompoundPhysicalSpan>, start: u64, length: usize, role: SpanRole) {
    if length == 0 {
        return;
    }
    spans.push(CompoundPhysicalSpan {
        start,
        end: start + length as u64,
        role,
    });
}

fn sector_range(
    sector_size: usize,
    count: usize,
    bytes_len: usize,
    id: u32,
) -> Result<(usize, usize), CodecError> {
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
    if start >= bytes_len {
        return malformed("CFB sector is absent");
    }
    let end = start
        .checked_add(sector_size)
        .ok_or_else(|| CodecError::Malformed("CFB sector offset overflow".into()))?
        .min(bytes_len);
    Ok((start, end))
}

fn sector_slice(bytes: &[u8], sector_size: usize, count: usize, id: u32) -> Option<&[u8]> {
    let (start, end) = sector_range(sector_size, count, bytes.len(), id).ok()?;
    bytes.get(start..end)
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    View::u16_le_at(bytes, offset)
}
fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    View::u32_le_at(bytes, offset)
}
fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    View::u64_le_at(bytes, offset)
}
fn malformed<T>(message: impl Into<String>) -> Result<T, CodecError> {
    Err(CodecError::Malformed(message.into()))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::*;

    const SECTOR_SIZE: usize = 512;

    struct CountingReader {
        inner: &'static [u8],
        bytes_read: usize,
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.inner.read(buffer)?;
            self.bytes_read += read;
            Ok(read)
        }
    }

    #[test]
    fn detection_prefix_never_reads_past_its_byte_limit() {
        let mut source = CountingReader {
            inner: &[b'x'; 1024],
            bytes_read: 0,
        };

        let prefix = read_detection_prefix(&mut source, 128 * 1024, 16)
            .expect("a capped non-CFB prefix is not a size refusal");

        assert_eq!(prefix, vec![b'x'; 16]);
        assert_eq!(source.bytes_read, 16);
    }

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
    fn snapshot_opens_a_stream_from_a_partial_final_sector() {
        let file = partial_regular_fixture();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic CFB parses");
        let stream = snapshot
            .open(
                &ctx,
                snapshot
                    .stream("Store/Large")
                    .expect("regular stream exists"),
            )
            .expect("regular stream opens through the partial sector");
        assert_eq!(stream.window().len(), 4110);
        assert!(stream.window().iter().all(|byte| *byte == 0x5a));
        assert_eq!(
            snapshot
                .physical_ledger()
                .expect("physical ledger builds")
                .last()
                .expect("ledger contains the partial sector")
                .end,
            file.len() as u64
        );

        let mut too_large = partial_regular_fixture();
        sector_mut(&mut too_large, 0)[3 * 128 + 120..4 * 128]
            .copy_from_slice(&4608_u64.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&too_large, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("metadata still parses");
        assert!(snapshot
            .open(
                &ctx,
                snapshot
                    .stream("Store/Large")
                    .expect("regular stream exists")
            )
            .is_err());
    }

    #[test]
    fn rejects_a_partial_structural_sector() {
        let mut file = fixture();
        file.truncate(SECTOR_SIZE * 12 + 37);
        assert!(!snapshot_parses(&file));
    }

    #[test]
    fn snapshot_rejects_stream_handles_from_another_snapshot() {
        let file = fixture();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let first = CompoundSnapshot::new(&ctx, root).expect("first CFB snapshot parses");
        let second = CompoundSnapshot::new(&ctx, root).expect("second CFB snapshot parses");
        let foreign = second.stream("Small").expect("foreign stream exists");
        assert!(first.open(&ctx, foreign).is_err());

        let owned = first.stream("Small").expect("owned stream exists").clone();
        assert_eq!(
            first
                .open(&ctx, &owned)
                .expect("owned clone opens")
                .window(),
            b"small"
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
    fn prefix_probe_follows_available_difat_sectors() {
        let mut file = fixture();
        file.resize(file.len() + SECTOR_SIZE, 0xff);
        put_u32(&mut file, 68, 12);
        put_u32(&mut file, 72, 1);
        put_u32(sector_mut(&mut file, 11), 12 * 4, DIFAT_SECTOR);
        put_u32(sector_mut(&mut file, 12), SECTOR_SIZE - 4, END_OF_CHAIN);
        assert!(matches!(
            CompoundPrefixProbe::inspect(&file),
            CompoundPrefixProbe::DirectoryEvidence(_)
        ));
        assert_eq!(
            CompoundPrefixProbe::inspect(&file[..SECTOR_SIZE * 13]),
            CompoundPrefixProbe::Incomplete
        );
    }

    #[test]
    fn prefix_probe_uses_available_leading_fat_coverage() {
        let mut prefix = fixture();
        put_u32(&mut prefix, 44, 2);
        put_u32(&mut prefix, 80, 1_000);
        assert!(matches!(
            CompoundPrefixProbe::inspect(&prefix),
            CompoundPrefixProbe::DirectoryEvidence(_)
        ));
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

    #[test]
    fn accepts_non_semantic_directory_color_variants() {
        let mut file = fixture();
        let directory = sector_mut(&mut file, 0);
        directory[67] = 0;
        directory[2 * 128 + 67] = 1;
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let snapshot = CompoundSnapshot::new(&ctx, root)
            .expect("root color and black-height metadata do not govern traversal");
        assert!(snapshot.stream("Store/Large").is_some());
    }

    #[test]
    fn parses_v4_header_directory_and_full_stream_size() {
        let file = fixture_v4();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic CFB v4 parses");
        assert_eq!(snapshot.major_version(), 4);
        assert_eq!(snapshot.sector_size(), 4096);
        assert_eq!(
            snapshot
                .open(&ctx, snapshot.stream("Wide").expect("stream exists"))
                .expect("stream opens")
                .window(),
            vec![0x6d; 4096]
        );

        let mut directory = vec![0_u8; 4096];
        initialize_empty_directory_entries(&mut directory);
        directory_entry(
            &mut directory,
            0,
            "Root Entry",
            5,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0x1_0000_0001,
        );
        assert_eq!(
            parse_directory(None, &directory, 4).expect("v4 directory parses")[0].size,
            0x1_0000_0001
        );
        assert_eq!(
            parse_directory(None, &directory, 3).expect("v3 directory parses")[0].size,
            1
        );
    }

    #[test]
    fn v4_requires_zero_header_padding() {
        let mut file = fixture_v4();
        file[512] = 1;
        assert!(matches!(
            CompoundPrefixProbe::inspect(&file),
            CompoundPrefixProbe::Malformed(_)
        ));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        assert!(CompoundSnapshot::new(&ctx, root).is_err());
    }

    #[test]
    fn prefix_probe_requires_the_same_header_invariants_as_full_parse() {
        for (offset, value) in [(32, 5_u16), (56, 512_u16)] {
            let mut file = fixture();
            if offset == 32 {
                put_u16(&mut file, offset, value);
            } else {
                put_u32(&mut file, offset, u32::from(value));
            }
            assert!(matches!(
                CompoundPrefixProbe::inspect(&file),
                CompoundPrefixProbe::Malformed(_)
            ));
        }

        let mut file = fixture();
        file[34] = 1;
        assert!(matches!(
            CompoundPrefixProbe::inspect(&file),
            CompoundPrefixProbe::Malformed(_)
        ));
    }

    #[test]
    fn transaction_signature_is_not_a_structural_rejection() {
        let mut file = fixture();
        put_u32(&mut file, 52, 17);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        CompoundSnapshot::new(&ctx, root).expect("transaction signature is admitted");
    }

    #[test]
    fn rejects_allocated_sectors_without_an_owner() {
        let mut file = fixture();
        file.resize(file.len() + SECTOR_SIZE, 0);
        put_u32(sector_mut(&mut file, 11), 12 * 4, END_OF_CHAIN);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        assert!(CompoundSnapshot::new(&ctx, root).is_err());
    }

    #[test]
    fn locates_the_v4_range_lock_sector_only_above_two_gibibytes() {
        assert_eq!(
            range_lock_sector(4, 4096, RANGE_LOCK_END + 4096).expect("range lock computes"),
            Some(0x0007_fffe)
        );
        assert_eq!(
            range_lock_sector(4, 4096, RANGE_LOCK_END).expect("range lock computes"),
            None
        );
        assert_eq!(
            range_lock_sector(3, 512, RANGE_LOCK_END + 512).expect("v3 has no range lock"),
            None
        );
    }

    #[test]
    fn directory_keys_use_length_preserving_simple_uppercase_units() {
        assert_eq!(cfb_name_cmp("alpha", "ALPHA"), Ordering::Equal);
        assert_eq!(path_key("Store/alpha"), path_key("store/ALPHA"));
        assert_ne!(path_key("ß"), path_key("SS"));
        assert_eq!(cfb_upper_unit(0xd800), 0xd800);
    }

    #[test]
    fn accepts_stale_unallocated_directory_fields() {
        let mut directory = vec![0_u8; 128];
        directory[68..80].fill(0xff);
        directory[8] = 1;
        let entries = parse_directory(None, &directory, 3).expect("unallocated slot is skipped");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].object_type, 0);
        assert_eq!(entries[0].left, NO_STREAM);
        assert_eq!(entries[0].start_sector, FREE_SECTOR);
    }

    #[test]
    fn rejects_invalid_names_types_ordering_and_reachability() {
        let mut invalid_name = fixture();
        sector_mut(&mut invalid_name, 0)[128] = b'/';
        assert!(!snapshot_parses(&invalid_name));

        let mut invalid_utf16 = fixture();
        put_u16(sector_mut(&mut invalid_utf16, 0), 128, 0xd800);
        put_u16(sector_mut(&mut invalid_utf16, 0), 128 + 2, 0);
        put_u16(sector_mut(&mut invalid_utf16, 0), 128 + 64, 4);
        assert!(!snapshot_parses(&invalid_utf16));

        let mut invalid_type = fixture();
        sector_mut(&mut invalid_type, 0)[128 + 66] = 3;
        assert!(!snapshot_parses(&invalid_type));

        let mut duplicate_name = fixture();
        directory_entry(
            sector_mut(&mut duplicate_name, 0),
            2,
            "Small",
            1,
            NO_STREAM,
            NO_STREAM,
            3,
            0,
            0,
        );
        assert!(!snapshot_parses(&duplicate_name));

        let mut unreachable = fixture();
        put_u32(sector_mut(&mut unreachable, 0), 2 * 128 + 76, NO_STREAM);
        assert!(!snapshot_parses(&unreachable));
    }

    #[test]
    fn rejects_invalid_empty_truncated_and_unowned_mini_allocations() {
        let mut invalid_empty = fixture();
        sector_mut(&mut invalid_empty, 0)[128 + 120..128 + 128].fill(0);
        assert!(!snapshot_parses(&invalid_empty));

        let mut truncated = fixture();
        sector_mut(&mut truncated, 0)[3 * 128 + 120..4 * 128]
            .copy_from_slice(&4608_u64.to_le_bytes());
        assert!(!snapshot_parses(&truncated));

        let mut unowned_mini = fixture();
        put_u32(sector_mut(&mut unowned_mini, 10), 4, END_OF_CHAIN);
        assert!(!snapshot_parses(&unowned_mini));

        let mut beyond_root_size = fixture();
        sector_mut(&mut beyond_root_size, 0)[120..128].copy_from_slice(&5_u64.to_le_bytes());
        sector_mut(&mut beyond_root_size, 0)[128 + 120..128 + 128]
            .copy_from_slice(&64_u64.to_le_bytes());
        assert!(!snapshot_parses(&beyond_root_size));
    }

    #[test]
    fn v3_ignores_the_uninitialized_stream_size_high_word() {
        let mut file = fixture();
        put_u32(sector_mut(&mut file, 0), 128 + 124, 0xdead_beef);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("synthetic CFB fits the decode policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("v3 high word is ignored");
        assert_eq!(
            snapshot
                .stream("Small")
                .expect("stream exists")
                .logical_size(),
            5
        );
    }

    #[test]
    fn snapshot_metadata_is_admitted_through_decode_budgets() {
        let file = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("root input fits its independent budget");
        let CodecError::ResourceLimit(limit) =
            CompoundSnapshot::new(&ctx, root).expect_err("retained metadata exceeds one byte")
        else {
            panic!("retained budget refusal is typed")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RetainedBytes
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("root input fits its independent budget");
        let CodecError::ResourceLimit(limit) = CompoundSnapshot::new(&ctx, root)
            .expect_err("allocation tables exceed one collection item")
        else {
            panic!("collection budget refusal is typed")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::CollectionItems
        );
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

    fn partial_regular_fixture() -> Vec<u8> {
        let mut file = fixture();
        file.resize(file.len() + SECTOR_SIZE, 0x5a);
        directory_entry(
            sector_mut(&mut file, 0),
            3,
            "Large",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            2,
            4110,
        );
        put_u32(sector_mut(&mut file, 11), 9 * 4, 12);
        put_u32(sector_mut(&mut file, 11), 12 * 4, END_OF_CHAIN);
        file.truncate(SECTOR_SIZE * 13 + 37);
        file
    }

    fn fixture_v4() -> Vec<u8> {
        const V4_SECTOR_SIZE: usize = 4096;
        let mut file = vec![0_u8; V4_SECTOR_SIZE * 4];
        file[..8].copy_from_slice(&MAGIC);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 4);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 12);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 40, 1);
        put_u32(&mut file, 44, 1);
        put_u32(&mut file, 48, 0);
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, END_OF_CHAIN);
        put_u32(&mut file, 68, END_OF_CHAIN);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, FREE_SECTOR);
        }
        put_u32(&mut file, 76, 2);

        let directory = sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 0);
        initialize_empty_directory_entries(directory);
        directory_entry(
            directory,
            0,
            "Root Entry",
            5,
            NO_STREAM,
            NO_STREAM,
            1,
            END_OF_CHAIN,
            0,
        );
        directory_entry(
            directory, 1, "Wide", 2, NO_STREAM, NO_STREAM, NO_STREAM, 1, 4096,
        );
        sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 1).fill(0x6d);
        let fat = sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 2);
        fat.fill(0xff);
        put_u32(fat, 0, END_OF_CHAIN);
        put_u32(fat, 4, END_OF_CHAIN);
        put_u32(fat, 8, FAT_SECTOR);
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
        sector_mut_with_size(file, SECTOR_SIZE, id)
    }

    fn sector_mut_with_size(file: &mut [u8], sector_size: usize, id: usize) -> &mut [u8] {
        let start = sector_size * (id + 1);
        &mut file[start..start + sector_size]
    }

    fn initialize_empty_directory_entries(directory: &mut [u8]) {
        for entry in directory.chunks_exact_mut(128) {
            entry.fill(0);
            entry[68..80].fill(0xff);
        }
    }

    fn snapshot_parses(file: &[u8]) -> bool {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let Ok((ctx, root)) = DecodeContext::from_root_bytes(file, &arena, &policy) else {
            return false;
        };
        CompoundSnapshot::new(&ctx, root).is_ok()
    }
    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
