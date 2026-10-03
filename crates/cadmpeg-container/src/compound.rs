// SPDX-License-Identifier: Apache-2.0
//! Lazy, budgeted Microsoft Compound File Binary (CFB) snapshots.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use cadmpeg_core::decode::{ByteRange, DecodeContext, ScopedReservation, View};
use cadmpeg_core::{CodecError, ContainerEntry};

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const DIFAT_SECTOR: u32 = 0xffff_fffc;
// NO_STREAM and FREE_SECTOR are the CFB specification names for the same value.
const NO_STREAM: u32 = FREE_SECTOR;
const V3_MAX_FILE_SIZE: u64 = 0x8000_0000;
const RANGE_LOCK_START: u64 = 0x7fff_ff00;
const RANGE_LOCK_END: u64 = 0x8000_0000;
const MINI_SECTOR_SIZE: usize = 64;
const MINI_STREAM_CUTOFF: u64 = 4096;

static NEXT_COMPOUND_SNAPSHOT_ID: AtomicU64 = AtomicU64::new(1);

/// Stable identity for a CFB storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompoundStorageId(u32);

impl CompoundStorageId {
    /// Returns the CFB directory-entry index.
    pub const fn directory_id(self) -> u32 {
        self.0
    }
}

/// Stable identity for a CFB stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompoundStreamId(u32);

impl cadmpeg_core::decode::cost::DecodeCost for CompoundStreamId {
    const FIXED_BYTES: Option<u64> = Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Self>()));
    fn decode_cost(&self, _ctx: &cadmpeg_core::decode::DecodeContext<'_>, _operation: &'static str) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Self>()))
    }
}


impl CompoundStreamId {
    /// Returns the CFB directory-entry index.
    pub const fn directory_id(self) -> u32 {
        self.0
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
    data: StreamData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EmptyStreamStart {
    EndOfChain,
    FreeSector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SectorChain {
    first: u32,
    rest: Vec<u32>,
}

impl SectorChain {
    fn len(&self) -> usize {
        1 + self.rest.len()
    }

    fn admitted<'sectors>(&'sectors self, ctx: &DecodeContext<'_>) -> Result<impl Iterator<Item = &'sectors u32> + 'sectors, CodecError> {
        ctx.charge_work(1, "visit CFB first chain sector")?;
        Ok(Some(&self.first).into_iter().chain(ctx.admit_iter(&self.rest, "visit CFB chain sectors")?))
    }

    fn get(&self, index: usize) -> Option<&u32> {
        match index.checked_sub(1) {
            None => Some(&self.first),
            Some(index) => self.rest.get(index),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamData {
    Empty(EmptyStreamStart),
    Allocated {
        logical_size: NonZeroU64,
        allocation: CompoundAllocation,
        chain: SectorChain,
    },
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
        match &self.data {
            StreamData::Empty(_) => 0,
            StreamData::Allocated { logical_size, .. } => logical_size.get(),
        }
    }

    /// Returns the first sector or the source marker for an empty stream.
    pub const fn start_sector(&self) -> u32 {
        match &self.data {
            StreamData::Empty(EmptyStreamStart::EndOfChain) => END_OF_CHAIN,
            StreamData::Empty(EmptyStreamStart::FreeSector) => FREE_SECTOR,
            StreamData::Allocated { chain, .. } => chain.first,
        }
    }

    /// Returns the allocation mechanism for a nonempty stream.
    pub const fn allocation(&self) -> Option<CompoundAllocation> {
        match &self.data {
            StreamData::Empty(_) => None,
            StreamData::Allocated { allocation, .. } => Some(*allocation),
        }
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

#[derive(Debug, Clone)]
enum DirectorySlot {
    Free,
    Live(LiveEntry),
}

impl DirectorySlot {
    fn live(&self) -> Option<&LiveEntry> {
        match self {
            Self::Free => None,
            Self::Live(entry) => Some(entry),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectoryKind {
    Storage,
    Stream,
    Root,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectoryColor {
    Red,
    Black,
}

#[derive(Debug, Clone)]
struct DirectoryName(String);

impl DirectoryName {
    fn new(ctx: &DecodeContext<'_>, name: String) -> Result<Self, CodecError> {
        if name.is_empty() {
            return Err(CodecError::NotImplemented(
                "CFB empty live directory names cannot be represented as paths".into(),
            ));
        }
        if ctx.admit_iter(name.as_str(), "check CFB directory name width")?.encode_utf16().count() > 31
            || ctx.admit_iter(name.as_str(), "check CFB directory name characters")?
                .any(|character| matches!(character, '/' | '\\' | ':' | '!'))
        {
            return malformed(
                "CFB directory name contains a forbidden character or exceeds 31 UTF-16 units",
            );
        }
        Ok(Self(name))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
struct LiveEntry {
    name: DirectoryName,
    kind: DirectoryKind,
    color: DirectoryColor,
    left: u32,
    right: u32,
    child: u32,
    start_sector: u32,
    size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompoundVersion {
    V3,
    V4,
}

impl CompoundVersion {
    fn from_header(major: u16, sector_shift: u16) -> Option<Self> {
        match (major, sector_shift) {
            (3, 9) => Some(Self::V3),
            (4, 12) => Some(Self::V4),
            _ => None,
        }
    }

    const fn major(self) -> u16 {
        match self {
            Self::V3 => 3,
            Self::V4 => 4,
        }
    }

    const fn sector_size(self) -> usize {
        match self {
            Self::V3 => 512,
            Self::V4 => 4096,
        }
    }
}

#[derive(Debug)]
struct CompoundState {
    version: CompoundVersion,
    sector_count: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    directory: Vec<DirectorySlot>,
    directory_chain: Option<SectorChain>,
    mini_fat_chain: Option<SectorChain>,
    root_mini_chain: Option<SectorChain>,
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
        let entries = parsed.build_entries(ctx, snapshot_id)?;
        parsed.validate_sector_ownership(ctx, &entries)?;
        let mut by_path = BTreeMap::new();
        let mut streams_by_id = BTreeMap::new();
        for (index, entry) in ctx.admit_iter(&entries, "visit CFB indexed entries")?.enumerate() {
            let key = path_key(ctx, entry.path())?;
            if ctx
                .insert_btree_map(&mut by_path, key, index, "index CFB path")?
                .is_some()
            {
                return malformed(ctx.format_retained(format_args!("duplicate CFB path {}", entry.path()), "CFB duplicate path error")?);
            }
            if let CompoundEntry::Stream(stream) = entry {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                        CompoundStreamId,
                        usize,
                    )>()),
                    "retain CFB stream index",
                )?;
                ctx.insert_btree_map(&mut streams_by_id, stream.id(), index, "index CFB stream")?;
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
        self.parsed.version.major()
    }

    /// Returns the regular-sector size.
    pub const fn sector_size(&self) -> usize {
        self.parsed.version.sector_size()
    }

    /// Returns entries in stable directory traversal order.
    pub fn entries(&self) -> &[CompoundEntry] {
        &self.entries
    }

    /// Finds an entry by a case-insensitive CFB path key.
    pub fn entry(&self, ctx: &DecodeContext<'_>, path: &str) -> Result<Option<&CompoundEntry>, CodecError> {
        let (entry, storage) = ctx.with_scoped_storage("CFB path lookup key", || -> Result<_, CodecError> {
            let key = path_key(ctx, path)?;
            Ok(ctx.get_btree_map(&self.by_path, &key, "CFB path lookup")?.map(|index| &self.entries[*index]))
        })?;
        drop(storage);
        Ok(entry)
    }

    /// Finds a stream by a case-insensitive CFB path key.
    pub fn stream(&self, ctx: &DecodeContext<'_>, path: &str) -> Result<Option<&CompoundStreamEntry>, CodecError> {
        Ok(match self.entry(ctx, path)? {
            Some(CompoundEntry::Stream(entry)) => Some(entry),
            Some(CompoundEntry::Storage(_)) | None => None,
        })
    }

    /// Finds a stream by stable directory identity.
    pub fn stream_by_id(&self, ctx: &DecodeContext<'_>, id: CompoundStreamId) -> Result<Option<&CompoundStreamEntry>, CodecError> {
        Ok(ctx.get_btree_map(&self.streams_by_id, &id, "CFB stream id lookup")?.and_then(|index| match &self.entries[*index] {
            CompoundEntry::Stream(entry) => Some(entry),
            CompoundEntry::Storage(_) => None,
        }))
    }

    /// Opens one stream as a borrowed contiguous run or a budgeted joined view.
    pub fn open(
        &self,
        ctx: &DecodeContext<'a>,
        entry: &CompoundStreamEntry,
    ) -> Result<View<'a>, CodecError> {
        if entry.snapshot_id != self.snapshot_id {
            return malformed("CFB stream handle does not belong to this snapshot");
        }
        let StreamData::Allocated {
            logical_size,
            allocation,
            chain,
        } = &entry.data
        else {
            let start = cadmpeg_core::decode::u64_from_index(self.root.start());
            return ctx.register_slice(self.root, ByteRange { start, end: start });
        };
        let logical_size = usize::try_from(logical_size.get())
            .map_err(|_| CodecError::Malformed("CFB stream size does not fit memory".into()))?;
        let sector_view = |sector| match allocation {
            CompoundAllocation::Regular => self.regular_sector_view(sector),
            CompoundAllocation::Mini => self.mini_sector_view(sector),
        };
        ctx.charge_work(1, "scan CFB first stream sector view")?;
        let first = sector_view(chain.first)?;
        let mut end = first.end();
        let mut contiguous = true;
        for &sector in ctx.admit_iter(&chain.rest, "visit CFB stream sectors")? {
            let view = sector_view(sector)?;
            contiguous &= view.start() == end;
            end = view.end();
        }
        let opened = if contiguous {
            self.root.child(first.start(), end).ok_or_else(|| {
                CodecError::Malformed("CFB contiguous stream range escapes input".into())
            })?
        } else {
            let mut views = ctx.temporary_vec(chain.len(), "CFB stream sector views")?;
            ctx.reserve_capacity(&mut views.0, 1, "CFB stream view slot")?;
            views.0.push(first);
            let mut copy_bytes = first.window().len();
            for &sector in ctx.admit_iter(&chain.rest, "visit CFB stream sectors")? {
                let view = sector_view(sector)?;
                copy_bytes = copy_bytes.checked_add(view.window().len()).ok_or_else(|| {
                    ctx.refuse_codec_limit("copy CFB stream sectors", u64::MAX, u64::MAX)
                })?;
                ctx.reserve_capacity(&mut views.0, 1, "CFB stream view slot")?;
                views.0.push(view);
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(copy_bytes),
                "copy CFB stream sectors",
            )?;
            ctx.concat_views(&views.0)?
        };
        let logical_end = opened
            .start()
            .checked_add(logical_size)
            .ok_or_else(|| CodecError::Malformed("CFB logical stream end overflow".into()))?;
        let opened = opened.child(opened.start(), logical_end).ok_or_else(|| {
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
        ctx: &DecodeContext<'_>,
        classify: impl Fn(&CompoundEntry) -> ContainerRole,
    ) -> Result<Vec<ContainerEntry>, CodecError> {
        ctx.try_collect_retained_with(&self.entries, "CFB container summaries", |entry| {
            let mut attributes = BTreeMap::new();
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text("directory_id", "CFB summary attribute key")?,
                ctx.format_retained(format_args!("{}", entry.directory_id()), "CFB summary attribute value")?,
                "CFB summary attributes",
            )?;
            let storage = match entry {
                CompoundEntry::Storage(_) => EntryStorage::Directory,
                CompoundEntry::Stream(stream) => {
                    if let Some(allocation) = stream.allocation() {
                        ctx.insert_btree_map(
                            &mut attributes,
                            ctx.copy_retained_text("allocation", "CFB summary attribute key")?,
                            ctx.copy_retained_text(allocation.label(), "CFB summary attribute value")?,
                            "CFB summary attributes",
                        )?;
                    }
                    ctx.insert_btree_map(
                        &mut attributes,
                        ctx.copy_retained_text("start_sector", "CFB summary attribute key")?,
                        ctx.format_retained(format_args!("{}", stream.start_sector()), "CFB summary attribute value")?,
                        "CFB summary attributes",
                    )?;
                    EntryStorage::verbatim(VerbatimLabel::Stored, stream.logical_size())
                }
            };
            Ok(ContainerEntry {
                name: ctx.copy_retained_text(entry.path(), "CFB summary entry name")?,
                role: classify(entry),
                storage,
                attributes,
            })
        })
    }

    fn regular_sector_view(&self, sector: u32) -> Result<View<'a>, CodecError> {
        let (start, end) = sector_range(
            self.parsed.version.sector_size(),
            self.parsed.sector_count,
            self.root.window().len(),
            sector,
        )?;
        let start =
            self.root.start().checked_add(start).ok_or_else(|| {
                CodecError::Malformed("CFB absolute sector start overflows".into())
            })?;
        let end = self
            .root
            .start()
            .checked_add(end)
            .ok_or_else(|| CodecError::Malformed("CFB absolute sector end overflows".into()))?;
        self.root
            .child(start, end)
            .ok_or_else(|| CodecError::Malformed("CFB sector escapes input".into()))
    }

    fn mini_sector_view(&self, mini_sector: u32) -> Result<View<'a>, CodecError> {
        let offset = usize::try_from(mini_sector)
            .ok()
            .and_then(|id| id.checked_mul(MINI_SECTOR_SIZE))
            .ok_or_else(|| CodecError::Malformed("CFB mini-sector offset overflow".into()))?;
        let regular_ordinal = offset / self.parsed.version.sector_size();
        let within = offset % self.parsed.version.sector_size();
        let &regular_sector = self
            .parsed
            .root_mini_chain
            .as_ref()
            .and_then(|chain| chain.get(regular_ordinal))
            .ok_or_else(|| {
                CodecError::Malformed("CFB mini sector escapes the root mini stream".into())
            })?;
        let sector = self.regular_sector_view(regular_sector)?;
        sector
            .child(
                sector.start() + within,
                sector.start() + within + MINI_SECTOR_SIZE,
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
        let version =
            CompoundVersion::from_header(major_version, sector_shift).ok_or_else(|| {
                CodecError::Malformed("unsupported or invalid CFB sector layout".into())
            })?;
        if le_u16(bytes, 32) != Some(6) || bytes.get(34..40) != Some(&[0; 6]) {
            return malformed("unsupported or invalid CFB sector layout");
        }
        let sector_size = version.sector_size();
        if bytes.len() < sector_size {
            return malformed("CFB input does not contain a complete header sector");
        }
        let sector_count = (bytes.len() - sector_size).div_ceil(sector_size);
        if sector_count < 2 {
            return malformed("CFB file has fewer than the minimum three sectors");
        }
        if version == CompoundVersion::V3
            && cadmpeg_core::decode::u64_from_index(bytes.len()) > V3_MAX_FILE_SIZE
        {
            return malformed("CFB v3 file exceeds the 2 GiB size ceiling");
        }
        if version == CompoundVersion::V4 && ctx.admit_iter(&bytes[512..sector_size], "check CFB header padding")?.any(|byte| *byte != 0) {
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
        if (version == CompoundVersion::V3 && directory_sector_count != 0)
            || (version == CompoundVersion::V4 && directory_sector_count == 0)
            || mini_stream_cutoff != MINI_STREAM_CUTOFF
            || fat_count == 0
            || fat_count > sector_count
            || difat_count > sector_count
        {
            return malformed("invalid CFB header counts or reserved fields");
        }
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(fat_count),
            "parse CFB allocation tables",
        )?;
        let mut allocation_id_scratch =
            ctx.reserve_scoped(0, "collect CFB allocation sector ids")?;
        let sector = |id| sector_slice(bytes, sector_size, sector_count, id);
        let mut fat_sectors = allocation_id_scratch
            .with_storage(|| ctx.vector_storage(fat_count, "CFB FAT sectors"))?;
        let mut header_free_seen = false;
        for index in 0..109 {
            let id = field(76 + index * 4, "header DIFAT entry")?;
            if id == FREE_SECTOR {
                header_free_seen = true;
            } else {
                if header_free_seen {
                    return malformed("non-free CFB header DIFAT entry follows a free entry");
                }
                if fat_sectors.len() == fat_count {
                    return malformed("CFB FAT ids exceed the declared count");
                }
                ctx.reserve_capacity(&mut fat_sectors, 1, "CFB FAT sector slot")?;
                fat_sectors.push(id);
            }
        }
        let mut next_difat = difat_start;
        let difat_entries = sector_size / 4 - 1;
        let mut seen_difat = BTreeSet::new();
        for _ in 0..difat_count {
            ctx.charge_work(1, "visit CFB DIFAT sectors")?;
            if cadmpeg_core::decode::index_from_u32(next_difat) >= sector_count {
                return malformed("CFB DIFAT chain is cyclic or out of range");
            }
            if !ctx.insert_btree_set(&mut seen_difat, next_difat, "collect CFB DIFAT sector ids")? {
                return malformed("CFB DIFAT chain is cyclic or out of range");
            }
            let data = sector(next_difat)
                .ok_or_else(|| CodecError::Malformed("CFB DIFAT sector is absent".into()))?;
            let mut free_seen = false;
            for index in 0..difat_entries {
                ctx.charge_work(1, "visit CFB DIFAT entries")?;
                let id = le_u32(data, index * 4)
                    .ok_or_else(|| CodecError::Malformed("truncated CFB DIFAT sector".into()))?;
                if id == FREE_SECTOR {
                    free_seen = true;
                } else {
                    if free_seen {
                        return malformed("non-free CFB DIFAT entry follows a free entry");
                    }
                    if fat_sectors.len() == fat_count {
                        return malformed("CFB FAT ids exceed the declared count");
                    }
                    ctx.reserve_capacity(&mut fat_sectors, 1, "CFB FAT sector slot")?;
                    fat_sectors.push(id);
                }
            }
            next_difat = le_u32(data, difat_entries * 4)
                .ok_or_else(|| CodecError::Malformed("truncated CFB DIFAT link".into()))?;
        }
        if (difat_count == 0 && difat_start != END_OF_CHAIN)
            || (difat_count != 0 && next_difat != END_OF_CHAIN)
            || fat_sectors.len() != fat_count
            || ctx.admit_iter(&fat_sectors, "check CFB FAT sector bounds")?
                .any(|id| cadmpeg_core::decode::index_from_u32(*id) >= sector_count)
        {
            return malformed("CFB DIFAT does not match its declared FAT count");
        }
        let fat_sector_set =
            ctx.collect_btree_set(ctx.admit_iter(&fat_sectors, "visit CFB FAT sector set")?.copied(), "collect CFB FAT sector set")?;
        if fat_sector_set.len() != fat_sectors.len() {
            return malformed("duplicate CFB FAT sector");
        }
        if !ctx.is_disjoint_btree_set(&fat_sector_set, &seen_difat, "check CFB table role overlap")? {
            return malformed("CFB sector has both FAT and DIFAT roles");
        }
        let fat_word_count = fat_count
            .checked_mul(sector_size / 4)
            .ok_or_else(|| CodecError::Malformed("CFB FAT word count overflow".into()))?;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(fat_word_count),
            "parse CFB FAT words",
        )?;
        let mut fat = ctx.vector_storage(fat_word_count, "retain CFB FAT")?;
        for &id in ctx.admit_iter(&fat_sectors, "visit CFB FAT sectors")? {
            let data = sector(id)
                .ok_or_else(|| CodecError::Malformed("CFB FAT sector is absent".into()))?;
            if data.len() != sector_size {
                return malformed("CFB FAT sector is truncated");
            }
            // `sector_size` is 512 or 4096, both exact multiples of four;
            // the length check above proves this sector has that width.
            for &raw in ctx.admit_iter(data.as_chunks::<4>().0, "decode CFB FAT words")? {
                ctx.reserve_capacity(&mut fat, 1, "CFB FAT word slots")?;
                fat.push(le_u32_array(raw));
            }
        }
        if fat.len() < sector_count {
            return malformed("CFB FAT does not address every physical sector");
        }
        if ctx.admit_iter(&fat, "check CFB trailing FAT entries")?
            .skip(sector_count)
            .any(|entry| *entry != FREE_SECTOR)
        {
            return malformed("CFB FAT entries past end-of-file are not free");
        }
        if ctx.admit_iter(&fat_sectors, "check CFB FAT role markers")?
            .any(|id| fat.get(cadmpeg_core::decode::index_from_u32(*id)) != Some(&FAT_SECTOR))
            || ctx.admit_iter(&seen_difat, "check CFB DIFAT role markers")?
                .any(|id| fat.get(cadmpeg_core::decode::index_from_u32(*id)) != Some(&DIFAT_SECTOR))
        {
            return malformed("CFB allocation table sector has the wrong role marker");
        }
        drop(fat_sectors);
        drop(allocation_id_scratch);
        let range_lock_sector =
            range_lock_sector(version, cadmpeg_core::decode::u64_from_index(bytes.len()));
        if range_lock_sector.is_some_and(|id| {
            fat.get(cadmpeg_core::decode::index_from_u32(id)) != Some(&END_OF_CHAIN)
        }) {
            return malformed("CFB range lock sector is not allocated as an end-of-chain sector");
        }
        let directory_expected = match version {
            CompoundVersion::V3 => Some(ChainLength::Unbounded),
            CompoundVersion::V4 => {
                NonZeroUsize::new(directory_sector_count).map(ChainLength::Declared)
            }
        };
        let directory_chain = chain(
            ctx,
            &fat,
            sector_count,
            directory_start,
            directory_expected,
            ChainRole::Directory,
        )?;
        let mut directory_scratch = ctx.reserve_scoped(0, "assemble CFB directory sectors")?;
        let directory_bytes = directory_scratch.with_storage(|| {
            join_sectors(
                ctx,
                bytes,
                sector_size,
                sector_count,
                directory_chain.as_ref().map(|chain| (&chain.first, chain.rest.as_slice())),
            )
        })?;
        let directory = parse_directory(ctx, &directory_bytes, version)?;
        drop(directory_bytes);
        drop(directory_scratch);
        validate_root(ctx, &directory)?;
        let mini_fat_chain = chain(
            ctx,
            &fat,
            sector_count,
            mini_fat_start,
            NonZeroUsize::new(mini_fat_count).map(ChainLength::Declared),
            ChainRole::MiniFat,
        )?;
        let mini_fat_byte_count = mini_fat_chain
            .as_ref()
            .map_or(0, SectorChain::len)
            .checked_mul(sector_size)
            .ok_or_else(|| CodecError::Malformed("CFB mini FAT byte size overflow".into()))?;
        let mut mini_fat_scratch = ctx.reserve_scoped(0, "assemble CFB mini FAT sectors")?;
        let mini_fat_word_count = mini_fat_byte_count / 4;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(mini_fat_word_count),
            "parse CFB mini FAT words",
        )?;
        let mini_fat_bytes = mini_fat_scratch.with_storage(|| {
            join_sectors(
                ctx,
                bytes,
                sector_size,
                sector_count,
                mini_fat_chain.as_ref().map(|chain| (&chain.first, chain.rest.as_slice())),
            )
        })?;
        // Every joined sector passed the same exact-width proof above, so the
        // joined mini FAT is an exact sequence of four-byte words.
        let mut mini_fat = ctx.vector_storage(mini_fat_word_count, "retain CFB mini FAT")?;
        for &raw in ctx.admit_iter(mini_fat_bytes.as_chunks::<4>().0, "decode CFB mini FAT words")? {
            ctx.reserve_capacity(&mut mini_fat, 1, "CFB mini FAT word slots")?;
            mini_fat.push(le_u32_array(raw));
        }
        drop(mini_fat_bytes);
        drop(mini_fat_scratch);
        let root = directory_root(&directory)?;
        let root_sectors = usize::try_from(root.size)
            .map_err(|_| {
                CodecError::Malformed("CFB root mini-stream size does not fit memory".into())
            })?
            .div_ceil(sector_size);
        let root_mini_chain = chain(
            ctx,
            &fat,
            sector_count,
            root.start_sector,
            NonZeroUsize::new(root_sectors).map(ChainLength::Declared),
            ChainRole::RootMiniStream,
        )?;
        Ok(Self {
            version,
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

    fn build_entries(
        &self,
        ctx: &DecodeContext<'_>,
        snapshot_id: u64,
    ) -> Result<Vec<CompoundEntry>, CodecError> {
        let mut output = Vec::new();
        let mut reached = (
            BTreeSet::new(),
            ctx.reserve_scoped(0, "traverse CFB directory")?,
        );
        self.walk_tree(
            ctx,
            snapshot_id,
            directory_root(&self.directory)?.child,
            None,
            &mut reached,
            &mut output,
        )?;
        for (id, entry) in ctx.admit_iter(&self.directory, "check CFB directory reachability")?.enumerate().skip(1) {
            if matches!(entry, DirectorySlot::Live(_)) {
                let reachable = match u32::try_from(id) {
                    Ok(id) => ctx.contains_btree_set(&reached.0, &id, "check CFB reachable directory id")?,
                    Err(_) => false,
                };
                if !reachable { return malformed("CFB directory contains an unreachable live entry"); }
            }
        }
        Ok(output)
    }

    fn walk_tree(
        &self,
        ctx: &DecodeContext<'_>,
        snapshot_id: u64,
        root: u32,
        parent: Option<&str>,
        reached: &mut (BTreeSet<u32>, ScopedReservation<'_>),
        output: &mut Vec<CompoundEntry>,
    ) -> Result<(), CodecError> {
        if root == NO_STREAM {
            return Ok(());
        }
        let _depth = ctx.enter_nested("traverse CFB storage hierarchy")?;
        validate_sibling_tree(ctx, &self.directory, root)?;
        let mut pending = ctx.temporary_vec(1, "traverse CFB pending siblings")?;
        ctx.reserve_capacity(&mut pending.0, 1, "CFB pending sibling slot")?;
        pending.0.push(root);
        while let Some(id) = pending.0.pop() {
            ctx.charge_work(1, "traverse CFB directory")?;
            let entry = self
                .directory
                .get(cadmpeg_core::decode::index_from_u32(id))
                .and_then(DirectorySlot::live)
                .ok_or_else(|| {
                    CodecError::Malformed("CFB directory link is out of range".into())
                })?;
            if !ctx.insert_scoped_btree_set(
                &mut reached.1,
                &mut reached.0,
                id,
                "compare CFB reached nodes",
                "traverse CFB directory",
            )? {
                return malformed("CFB directory entry belongs to more than one storage");
            }
            if entry.right != NO_STREAM {
                ctx.push_scoped_vec(
                    &mut pending.1,
                    &mut pending.0,
                    entry.right,
                    "traverse CFB pending siblings",
                )?;
            }
            let path = match parent {
                None => ctx.format_retained(
                    format_args!("{}", entry.name.as_str()),
                    "retain CFB entry path",
                )?,
                Some(parent) => ctx.format_retained(
                    format_args!("{parent}/{}", entry.name.as_str()),
                    "retain CFB entry path",
                )?,
            };
            match entry.kind {
                DirectoryKind::Storage => {
                    let mut parent_scope = ctx.reserve_scoped(0, "hold CFB storage parent path")?;
                    let parent_path = ctx.copy_scoped_text(
                        &path,
                        &mut parent_scope,
                        "hold CFB storage parent path",
                    )?;
                    ctx.push_vec(
                        output,
                        CompoundEntry::Storage(CompoundStorageEntry {
                            id: CompoundStorageId(id),
                            path,
                        }),
                        "retain CFB storage entry",
                    )?;
                    self.walk_tree(
                        ctx,
                        snapshot_id,
                        entry.child,
                        Some(&parent_path),
                        reached,
                        output,
                    )?;
                }
                DirectoryKind::Stream => {
                    let allocation = if entry.size < MINI_STREAM_CUTOFF {
                        CompoundAllocation::Mini
                    } else {
                        CompoundAllocation::Regular
                    };
                    let (fat, count, width, role) = match allocation {
                        CompoundAllocation::Regular => (
                            &self.fat,
                            self.sector_count,
                            self.version.sector_size(),
                            ChainRole::Stream,
                        ),
                        CompoundAllocation::Mini => (
                            &self.mini_fat,
                            self.mini_fat.len(),
                            MINI_SECTOR_SIZE,
                            ChainRole::MiniStream,
                        ),
                    };
                    let empty = || {
                        StreamData::Empty(if entry.start_sector == FREE_SECTOR {
                            EmptyStreamStart::FreeSector
                        } else {
                            EmptyStreamStart::EndOfChain
                        })
                    };
                    let data = match NonZeroU64::new(entry.size) {
                        None => empty(),
                        Some(logical_size) => {
                            let expected = usize::try_from(logical_size.get())
                                .map_err(|_| {
                                    CodecError::Malformed(
                                        "CFB stream size does not fit memory".into(),
                                    )
                                })?
                                .div_ceil(width);
                            let sectors = chain(
                                ctx,
                                fat,
                                count,
                                entry.start_sector,
                                NonZeroUsize::new(expected).map(ChainLength::Declared),
                                role,
                            )?;
                            match sectors {
                                Some(chain) => StreamData::Allocated {
                                    allocation,
                                    logical_size,
                                    chain,
                                },
                                None => empty(),
                            }
                        }
                    };
                    ctx.push_vec(
                        output,
                        CompoundEntry::Stream(CompoundStreamEntry {
                            id: CompoundStreamId(id),
                            snapshot_id,
                            path,
                            data,
                        }),
                        "retain CFB stream entry",
                    )?;
                }
                DirectoryKind::Root => {
                    return malformed("root object appears in a storage child tree")
                }
            }
            if entry.left != NO_STREAM {
                ctx.push_scoped_vec(
                    &mut pending.1,
                    &mut pending.0,
                    entry.left,
                    "traverse CFB pending siblings",
                )?;
            }
        }
        Ok(())
    }

    fn validate_sector_ownership(
        &self,
        ctx: &DecodeContext<'_>,
        entries: &[CompoundEntry],
    ) -> Result<(), CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "validate CFB sector ownership")?;
        let mut used = BTreeSet::new();
        if let Some(sector) = self.range_lock_sector {
            ctx.charge_work(1, "validate CFB structural sector")?;
            ctx.insert_scoped_btree_set(
                &mut scratch,
                &mut used,
                sector,
                "compare CFB owned sectors",
                "validate CFB sector ownership",
            )?;
        }
        let mut claim_structural = |sector| -> Result<(), CodecError> {
            if !ctx.insert_scoped_btree_set(
                &mut scratch,
                &mut used,
                sector,
                "compare CFB owned sectors",
                "validate CFB sector ownership",
            )? {
                return malformed("CFB regular sector has duplicate structural ownership");
            }
            Ok(())
        };
        for &sector in ctx.admit_iter(&self.fat_sectors, "visit CFB owned FAT sectors")?
            .chain(ctx.admit_iter(&self.difat_sectors, "visit CFB owned DIFAT sectors")?) {
            claim_structural(sector)?;
        }
        for chain in [&self.directory_chain, &self.mini_fat_chain, &self.root_mini_chain] {
            if let Some(chain) = chain {
                for &sector in chain.admitted(ctx)? {
                    claim_structural(sector)?;
                }
            }
        }
        let mut mini_used = BTreeSet::new();
        let root_size = directory_root(&self.directory)?.size;
        let mini_capacity = usize::try_from(root_size)
            .map_err(|_| {
                CodecError::Malformed("CFB root mini-stream size does not fit memory".into())
            })?
            .div_ceil(MINI_SECTOR_SIZE);
        for entry in ctx.admit_iter(entries, "visit CFB stream ownership")? {
            if let CompoundEntry::Stream(stream) = entry {
                let StreamData::Allocated { allocation, chain, .. } = &stream.data else {
                    continue;
                };
                let allocation = *allocation;
                let target = if allocation == CompoundAllocation::Regular {
                    &mut used
                } else {
                    &mut mini_used
                };
                let mut remaining = stream.logical_size();
                for &sector in chain.admitted(ctx)? {
                    let payload =
                        remaining.min(cadmpeg_core::decode::u64_from_index(MINI_SECTOR_SIZE));
                    remaining -= payload;
                    if allocation == CompoundAllocation::Mini
                        && (cadmpeg_core::decode::index_from_u32(sector) >= mini_capacity
                            || u64::from(sector)
                                .checked_mul(cadmpeg_core::decode::u64_from_index(MINI_SECTOR_SIZE))
                                .and_then(|offset| offset.checked_add(payload))
                                .is_none_or(|end| end > root_size))
                    {
                        return malformed("CFB mini stream escapes the root mini stream");
                    }
                    if !ctx.insert_scoped_btree_set(
                        &mut scratch,
                        target,
                        sector,
                        "compare CFB owned sectors",
                        "validate CFB sector ownership",
                    )? {
                        return malformed("CFB stream sector has duplicate ownership");
                    }
                }
            }
        }
        for (sector, marker) in ctx.admit_iter(&self.mini_fat, "visit CFB mini FAT ownership")?.enumerate() {
            let sector = u32::try_from(sector)
                .map_err(|_| CodecError::Malformed("CFB mini-sector id exceeds u32".into()))?;
            if !ctx.contains_btree_set(&mini_used, &sector, "check CFB mini FAT ownership")? && *marker != FREE_SECTOR {
                return malformed("unowned CFB mini sector is not marked free");
            }
        }
        for (sector, marker) in ctx.admit_iter(&self.fat, "visit CFB FAT ownership")?.take(self.sector_count).enumerate() {
            let sector = u32::try_from(sector)
                .map_err(|_| CodecError::Malformed("CFB sector id exceeds u32".into()))?;
            if !ctx.contains_btree_set(&used, &sector, "check CFB FAT ownership")? && *marker != FREE_SECTOR {
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
    /// Parses complete prefix structures with caller-admitted scratch storage.
    pub fn inspect_with_context<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<(Self, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
        ctx.with_scoped_storage("CFB prefix probe", || {
            let prefix = prefix.window();
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(prefix.len()),
                "CFB probe input scan",
            )?;
            if prefix.get(..8) != Some(&MAGIC) {
                return Ok(Self::NotCompound);
            }
            let Some(major) = le_u16(prefix, 26) else {
                return Ok(Self::Incomplete);
            };
            let Some(shift) = le_u16(prefix, 30) else {
                return Ok(Self::Incomplete);
            };
            let Some(version) = CompoundVersion::from_header(major, shift) else {
                return Ok(Self::Malformed("invalid CFB sector layout".into()));
            };
            let sector_size = version.sector_size();
            if prefix.len() < sector_size {
                return Ok(Self::Incomplete);
            }
            if prefix.get(8..24) != Some(&[0; 16])
                || le_u16(prefix, 24) != Some(0x003e)
                || le_u16(prefix, 28) != Some(0xfffe)
                || le_u16(prefix, 32) != Some(6)
                || prefix.get(34..40) != Some(&[0; 6])
                || le_u32(prefix, 56) != Some(4096)
            {
                return Ok(Self::Malformed("invalid CFB header".into()));
            }
            if version == CompoundVersion::V4
                && ctx.admit_iter(&prefix[512..sector_size], "check CFB probe header padding")?.any(|byte| *byte != 0)
            {
                return Ok(Self::Malformed("CFB v4 header padding is not zero".into()));
            }
            let Some(fat_count) = le_u32(prefix, 44).and_then(|v| usize::try_from(v).ok()) else {
                return Ok(Self::Incomplete);
            };
            let Some(directory_start) = le_u32(prefix, 48) else {
                return Ok(Self::Incomplete);
            };
            let Some(directory_sector_count) = le_u32(prefix, 40) else {
                return Ok(Self::Incomplete);
            };
            let Some(difat_start) = le_u32(prefix, 68) else {
                return Ok(Self::Incomplete);
            };
            let Some(difat_count) = le_u32(prefix, 72).and_then(|v| usize::try_from(v).ok()) else {
                return Ok(Self::Incomplete);
            };
            if fat_count == 0
                || (version == CompoundVersion::V3 && directory_sector_count != 0)
                || (version == CompoundVersion::V4 && directory_sector_count == 0)
            {
                return Ok(Self::Malformed("invalid CFB header counts".into()));
            }
            let available = (prefix.len() - sector_size) / sector_size;
            ctx.charge_collection_items(
                cadmpeg_core::decode::u64_from_index(fat_count),
                "probe CFB FAT sectors",
            )?;
            let mut fat_sectors = ctx.vector_storage(fat_count, "probe CFB FAT sectors")?;
            let mut header_free_seen = false;
            ctx.charge_work(109, "CFB header DIFAT scan")?;
            for index in 0..109 {
                let Some(id) = le_u32(prefix, 76 + index * 4) else {
                    return Ok(Self::Incomplete);
                };
                if id == FREE_SECTOR {
                    header_free_seen = true;
                } else {
                    if header_free_seen {
                        return Ok(Self::Malformed(
                            "non-free CFB header DIFAT entry follows a free entry".into(),
                        ));
                    }
                    if fat_sectors.len() == fat_count {
                        return Ok(Self::Malformed(
                            "CFB FAT ids exceed the declared count".into(),
                        ));
                    }
                    ctx.reserve_capacity(&mut fat_sectors, 1, "CFB FAT sector slot")?;
                    fat_sectors.push(id);
                }
            }
            let mut next_difat = difat_start;
            let difat_entries = sector_size / 4 - 1;
            let mut seen_difat_storage = ctx.reserve_scoped(0, "CFB probe visits")?;
            let mut seen_difat = BTreeSet::new();
            for _ in 0..difat_count {
                ctx.charge_work(1, "visit CFB probe DIFAT sector")?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(sector_size),
                    "CFB probe DIFAT step",
                )?;
                if cadmpeg_core::decode::index_from_u32(next_difat) >= available {
                    return Ok(Self::Incomplete);
                }
                if !ctx.insert_scoped_btree_set(
                    &mut seen_difat_storage,
                    &mut seen_difat,
                    next_difat,
                    "CFB probe DIFAT visits",
                    "CFB probe DIFAT visits",
                )? {
                    return Ok(Self::Malformed("CFB DIFAT chain is cyclic".into()));
                }
                let Some(raw) = sector_slice(prefix, sector_size, available, next_difat) else {
                    return Ok(Self::Incomplete);
                };
                let mut free_seen = false;
                for index in 0..difat_entries {
                ctx.charge_work(1, "visit CFB DIFAT entries")?;
                    let Some(id) = le_u32(raw, index * 4) else {
                        return Ok(Self::Incomplete);
                    };
                    if id == FREE_SECTOR {
                        free_seen = true;
                    } else {
                        if free_seen {
                            return Ok(Self::Malformed(
                                "non-free CFB DIFAT entry follows a free entry".into(),
                            ));
                        }
                        if fat_sectors.len() == fat_count {
                            return Ok(Self::Malformed(
                                "CFB FAT ids exceed the declared count".into(),
                            ));
                        }
                        ctx.reserve_capacity(&mut fat_sectors, 1, "CFB FAT sector slot")?;
                        fat_sectors.push(id);
                    }
                }
                let Some(next) = le_u32(raw, difat_entries * 4) else {
                    return Ok(Self::Incomplete);
                };
                next_difat = next;
            }
            if (difat_count == 0 && difat_start != END_OF_CHAIN)
                || (difat_count != 0 && next_difat != END_OF_CHAIN)
            {
                return Ok(Self::Malformed(
                    "CFB DIFAT chain length does not match the header".into(),
                ));
            }
            if fat_sectors.len() != fat_count {
                return Ok(Self::Malformed(
                    "CFB DIFAT does not match its declared FAT count".into(),
                ));
            }
            let mut fat = Vec::new();
            let mut loaded_fat_count = 0;
            for &id in ctx.admit_iter(&fat_sectors, "visit CFB FAT sectors")? {
                if cadmpeg_core::decode::index_from_u32(id) >= available {
                    break;
                }
                let Some(raw) = sector_slice(prefix, sector_size, available, id) else {
                    return Ok(Self::Incomplete);
                };
                // `available` counts only complete sectors, and each admitted
                // sector width is an exact multiple of four.
                ctx.reserve_vec(&mut fat, raw.len() / 4, "CFB probe FAT words")?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(raw.len()),
                    "CFB probe FAT words",
                )?;
                for &word in ctx.admit_iter(raw.as_chunks::<4>().0, "decode CFB probe FAT words")? {
                    ctx.reserve_capacity(&mut fat, 1, "CFB probe FAT word slot")?;
                    fat.push(le_u32_array(word));
                }
                loaded_fat_count += 1;
            }
            if ctx.admit_iter(&fat_sectors, "check CFB probe FAT roles")?
                .take(loaded_fat_count)
                .any(|id| fat.get(cadmpeg_core::decode::index_from_u32(*id)) != Some(&FAT_SECTOR))
                || ctx.admit_iter(&seen_difat, "check CFB probe DIFAT roles")?.any(|id| {
                    fat.get(cadmpeg_core::decode::index_from_u32(*id))
                        .is_some_and(|role| role != &DIFAT_SECTOR)
                })
            {
                return Ok(Self::Malformed(
                    "CFB allocation sector has the wrong role marker".into(),
                ));
            }
            let expected_directory_count = if version == CompoundVersion::V4 {
                Some(cadmpeg_core::decode::index_from_u32(directory_sector_count))
            } else if directory_sector_count == 0 {
                None
            } else {
                return Ok(Self::Malformed(
                    "CFB v3 declares directory sector count".into(),
                ));
            };
            let mut directory_chain = Vec::new();
            let mut seen_directory_storage = ctx.reserve_scoped(0, "CFB probe visits")?;
            let mut seen_directory = BTreeSet::new();
            let mut current = directory_start;
            loop {
                ctx.charge_work(1, "visit CFB probe directory chain")?;
                if cadmpeg_core::decode::index_from_u32(current) >= available {
                    return Ok(Self::Incomplete);
                }
                let Some(&next) = fat.get(cadmpeg_core::decode::index_from_u32(current)) else {
                    return Ok(if loaded_fat_count < fat_count {
                        Self::Incomplete
                    } else {
                        Self::Malformed("CFB FAT does not address the directory sector".into())
                    });
                };
                if !ctx.insert_scoped_btree_set(
                    &mut seen_directory_storage,
                    &mut seen_directory,
                    current,
                    "CFB probe directory visits",
                    "CFB probe directory visits",
                )? {
                    return Ok(Self::Malformed("CFB directory chain is cyclic".into()));
                }
                ctx.push_vec(&mut directory_chain, current, "CFB probe directory chain")?;
                if next == END_OF_CHAIN {
                    break;
                }
                if next >= DIFAT_SECTOR {
                    return Ok(Self::Malformed(
                        "CFB directory chain has an invalid terminator".into(),
                    ));
                }
                current = next;
            }
            if expected_directory_count.is_some_and(|count| count != directory_chain.len()) {
                return Ok(Self::Malformed(
                    "CFB directory chain length does not match the header".into(),
                ));
            }
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(directory_chain.len() * sector_size),
                "CFB probe joined directory",
            )?;
            let directory_bytes =
                join_sectors(ctx, prefix, sector_size, available, directory_chain.split_first())?;
            let directory = match parse_directory(ctx, &directory_bytes, version) {
                Ok(value) => value,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => return Ok(Self::Malformed(ctx.format_retained(format_args!("{error}"), "CFB probe error")?)),
            };
            let root = match directory_root(&directory) {
                Ok(root) => root,
                Err(error) => return Ok(Self::Malformed(ctx.format_retained(format_args!("{error}"), "CFB probe error")?)),
            };
            if let Err(error) = validate_root(ctx, &directory) {
                if matches!(error, CodecError::ResourceLimit(_)) { return Err(error); }
                return Ok(Self::Malformed(ctx.format_retained(format_args!("{error}"), "CFB probe error")?));
            }
            if let Err(error) = validate_sibling_tree(ctx, &directory, root.child) {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                return Ok(Self::Malformed(ctx.format_retained(format_args!("{error}"), "CFB probe error")?));
            }
            let mut names = Vec::new();
            let mut pending = ctx.collection_vec(1, "CFB probe pending links")?;
            ctx.reserve_capacity(&mut pending, 1, "CFB probe pending slot")?;
            pending.push((root.child, String::new()));
            let mut seen_storage = ctx.reserve_scoped(0, "CFB probe visits")?;
            let mut seen = BTreeSet::new();
            while let Some((id, parent)) = pending.pop() {
                ctx.charge_work(1, "visit CFB probe path")?;
                if id == NO_STREAM {
                    continue;
                }
                let Some(entry) = directory
                    .get(cadmpeg_core::decode::index_from_u32(id))
                    .and_then(DirectorySlot::live)
                else {
                    return Ok(Self::Malformed("CFB directory link is out of range".into()));
                };
                if !ctx.insert_scoped_btree_set(
                    &mut seen_storage,
                    &mut seen,
                    id,
                    "CFB probe live entries",
                    "CFB probe live entries",
                )? {
                    return Ok(Self::Malformed("CFB directory link cycle".into()));
                }
                let path = if parent.is_empty() {
                    ctx.copy_retained_text(entry.name.as_str(), "CFB probe path")?
                } else {
                    ctx.format_retained(
                        format_args!("{parent}/{}", entry.name.as_str()),
                        "CFB probe path",
                    )?
                };
                ctx.push_vec(
                    &mut names,
                    ctx.copy_retained_text(&path, "CFB probe path copy")?,
                    "CFB probe paths",
                )?;
                ctx.push_vec(
                    &mut pending,
                    (
                        entry.left,
                        ctx.copy_retained_text(&parent, "CFB probe parent copy")?,
                    ),
                    "CFB probe pending links",
                )?;
                ctx.push_vec(
                    &mut pending,
                    (
                        entry.right,
                        ctx.copy_retained_text(&parent, "CFB probe parent copy")?,
                    ),
                    "CFB probe pending links",
                )?;
                if entry.kind == DirectoryKind::Storage {
                    if let Err(error) = validate_sibling_tree(ctx, &directory, entry.child) {
                        if matches!(error, CodecError::ResourceLimit(_)) {
                            return Err(error);
                        }
                        return Ok(Self::Malformed(ctx.format_retained(format_args!("{error}"), "CFB probe error")?));
                    }
                    ctx.push_vec(&mut pending, (entry.child, path), "CFB probe pending links")?;
                }
            }
            for (id, entry) in ctx.admit_iter(&directory, "visit CFB probe reachability")?.enumerate().skip(1) {
                if matches!(entry, DirectorySlot::Live(_)) {
                    let reachable = match u32::try_from(id) {
                        Ok(id) => ctx.contains_btree_set(&seen, &id, "check CFB probe reachable id")?,
                        Err(_) => false,
                    };
                    if !reachable {
                        return Ok(Self::Malformed(
                            "CFB directory contains an unreachable live entry".into(),
                        ));
                    }
                }
            }
            Ok(Self::DirectoryEvidence(names))
        })
    }

    /// Returns structurally reached paths when the prefix provides usable evidence.
    pub fn paths(&self) -> Option<&[String]> {
        match self {
            Self::DirectoryEvidence(paths) => Some(paths),
            _ => None,
        }
    }
}

/// Reads the bounded detection prefix in the caller's session.
/// CFB prefixes grow until the directory evidence settles or the input limit refuses.
pub fn read_detection_prefix<R: Read + ?Sized>(
    ctx: &DecodeContext<'_>,
    source: &mut R,
    prefix_len: usize,
) -> Result<Vec<u8>, CodecError> {
    let max_bytes = ctx.policy().limits.max_input_bytes;
    let phase_one_len = match usize::try_from(max_bytes) {
        Ok(max_bytes) => prefix_len.min(max_bytes),
        Err(_) => prefix_len,
    };
    let mut bytes = ctx.read_input_prefix(source, phase_one_len)?;
    loop {
        ctx.charge_work(1, "visit CFB detection prefix")?;
        let (probe, storage) =
            CompoundPrefixProbe::inspect_with_context(ctx, View::over_retained(&bytes))?;
        let incomplete = matches!(probe, CompoundPrefixProbe::Incomplete);
        drop((probe, storage));
        if !incomplete {
            return Ok(bytes);
        }
        let used = cadmpeg_core::decode::u64_from_index(bytes.len());
        if used >= max_bytes {
            if ctx.probe_input_end(source, "CFB detection end probe")? {
                return Err(ctx.refuse_input_limit(1, "CFB detection input"));
            }
            return Ok(bytes);
        }
        let remaining = usize::try_from(max_bytes - used)
            .map_or(64 * 1024, |remaining| remaining.min(64 * 1024));
        let previous = bytes.len();
        ctx.extend_input_prefix(source, &mut bytes, previous + remaining)?;
        if bytes.len() == previous {
            return Ok(bytes);
        }
    }
}

const DIRECTORY_NAME_LENGTH: usize = 64;
const DIRECTORY_LEFT: usize = 68;
const DIRECTORY_RIGHT: usize = 72;
const DIRECTORY_CHILD: usize = 76;
const DIRECTORY_START_SECTOR: usize = 116;
const DIRECTORY_SIZE: usize = 120;
const _: () = assert!(DIRECTORY_NAME_LENGTH.is_multiple_of(2));
const _: () = assert!(DIRECTORY_LEFT.is_multiple_of(4));
const _: () = assert!(DIRECTORY_RIGHT.is_multiple_of(4));
const _: () = assert!(DIRECTORY_CHILD.is_multiple_of(4));
const _: () = assert!(DIRECTORY_START_SECTOR.is_multiple_of(4));
const _: () = assert!(DIRECTORY_SIZE.is_multiple_of(8));

fn parse_directory(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    version: CompoundVersion,
) -> Result<Vec<DirectorySlot>, CodecError> {
    let (records, remainder) = bytes.as_chunks::<128>();
    if !remainder.is_empty() {
        return malformed("CFB directory stream has a partial entry");
    }
    let entry_count = records.len();
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(entry_count),
        "parse CFB directory entries",
    )?;
    let mut entries = ctx.vector_storage(entry_count, "parse CFB directory entries")?;
    for raw in ctx.admit_iter(records, "visit CFB directory records")? {
        let object_type = raw[66];
        if object_type == 0 {
            ctx.reserve_capacity(&mut entries, 1, "CFB directory record slot")?;
            entries.push(DirectorySlot::Free);
            continue;
        }
        let kind = match object_type {
            1 => DirectoryKind::Storage,
            2 => DirectoryKind::Stream,
            5 => DirectoryKind::Root,
            _ => return malformed("invalid CFB directory object type"),
        };
        let name_len = usize::from(le_u16_array(
            raw.as_chunks::<2>().0[DIRECTORY_NAME_LENGTH / 2],
        ));
        let name = {
            if !(2..=64).contains(&name_len)
                || !name_len.is_multiple_of(2)
                || raw[name_len - 2..name_len] != [0, 0]
            {
                return malformed("invalid CFB directory name length or terminator");
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(name_len) * 2,
                "decode and check CFB directory name",
            )?;
            let name =
                ctx.utf16le_text(raw, (name_len - 2) / 2, false, "decode CFB directory name")?;
            DirectoryName::new(ctx, name)?
        };
        let color = match raw[67] {
            0 => DirectoryColor::Red,
            1 => DirectoryColor::Black,
            _ => return malformed("invalid CFB directory node color"),
        };
        let mut size = le_u64_array(raw.as_chunks::<8>().0[DIRECTORY_SIZE / 8]);
        if version == CompoundVersion::V3 {
            size &= 0xffff_ffff;
        }
        let entry = DirectorySlot::Live(LiveEntry {
            name,
            kind,
            color,
            left: le_u32_array(raw.as_chunks::<4>().0[DIRECTORY_LEFT / 4]),
            right: le_u32_array(raw.as_chunks::<4>().0[DIRECTORY_RIGHT / 4]),
            child: le_u32_array(raw.as_chunks::<4>().0[DIRECTORY_CHILD / 4]),
            start_sector: le_u32_array(raw.as_chunks::<4>().0[DIRECTORY_START_SECTOR / 4]),
            size,
        });
        ctx.reserve_capacity(&mut entries, 1, "CFB directory record slot")?;
        entries.push(entry);
    }
    Ok(entries)
}

fn directory_root(directory: &[DirectorySlot]) -> Result<&LiveEntry, CodecError> {
    directory
        .first()
        .ok_or_else(|| CodecError::Malformed("empty CFB directory".into()))?
        .live()
        .ok_or_else(|| CodecError::Malformed("invalid CFB root directory entry".into()))
}

fn validate_root(ctx: &DecodeContext<'_>, directory: &[DirectorySlot]) -> Result<(), CodecError> {
    let root = directory_root(directory)?;
    if root.kind != DirectoryKind::Root
        || root.name.as_str() != "Root Entry"
        || root.left != NO_STREAM
        || root.right != NO_STREAM
    {
        return malformed("invalid CFB root directory entry");
    }
    if ctx.admit_iter(directory, "scan CFB root entries")?.skip(1).any(|entry| {
        entry
            .live()
            .is_some_and(|entry| entry.kind == DirectoryKind::Root)
    }) {
        return malformed("CFB directory has more than one root entry");
    }
    Ok(())
}

fn validate_sibling_tree(
    ctx: &DecodeContext<'_>,
    directory: &[DirectorySlot],
    root: u32,
) -> Result<(), CodecError> {
    if root == NO_STREAM {
        return Ok(());
    }
    let root_entry = directory
        .get(cadmpeg_core::decode::index_from_u32(root))
        .ok_or_else(|| CodecError::Malformed("CFB sibling root is out of range".into()))?;
    let root_entry = root_entry.live().ok_or_else(|| {
        CodecError::Malformed("CFB sibling-tree root points to a free directory slot".into())
    })?;
    if root_entry.color != DirectoryColor::Black {
        return malformed("CFB sibling-tree root is not black");
    }
    let mut seen = (
        BTreeSet::new(),
        ctx.reserve_scoped(0, "validate CFB sibling nodes")?,
    );
    visit_sibling_tree(ctx, directory, root, None, None, false, &mut seen)
}

fn visit_sibling_tree(
    ctx: &DecodeContext<'_>,
    directory: &[DirectorySlot],
    id: u32,
    lower: Option<&str>,
    upper: Option<&str>,
    parent_red: bool,
    seen: &mut (BTreeSet<u32>, ScopedReservation<'_>),
) -> Result<(), CodecError> {
    if id == NO_STREAM {
        return Ok(());
    }
    let _depth = ctx.enter_nested("validate CFB sibling tree")?;
    ctx.charge_work(1, "visit CFB sibling node")?;
    let entry = directory
        .get(cadmpeg_core::decode::index_from_u32(id))
        .ok_or_else(|| CodecError::Malformed("CFB sibling link is out of range".into()))?;
    let Some(entry) = entry.live() else {
        return malformed("CFB sibling tree contains an invalid node or cycle");
    };
    if !matches!(entry.kind, DirectoryKind::Storage | DirectoryKind::Stream)
        || !ctx.insert_scoped_btree_set(
            &mut seen.1,
            &mut seen.0,
            id,
            "compare CFB sibling nodes",
            "validate CFB sibling nodes",
        )?
    {
        return malformed("CFB sibling tree contains an invalid node or cycle");
    }
    if lower.map(|name| cfb_name_cmp(ctx, name, entry.name.as_str())).transpose()?.is_some_and(|order| order != Ordering::Less)
        || upper.map(|name| cfb_name_cmp(ctx, entry.name.as_str(), name)).transpose()?.is_some_and(|order| order != Ordering::Less)
    {
        return malformed("CFB sibling tree violates directory-name ordering");
    }
    let red = entry.color == DirectoryColor::Red;
    if red && parent_red {
        return malformed("CFB sibling tree contains adjacent red nodes");
    }
    visit_sibling_tree(
        ctx,
        directory,
        entry.left,
        lower,
        Some(entry.name.as_str()),
        red,
        seen,
    )?;
    visit_sibling_tree(
        ctx,
        directory,
        entry.right,
        Some(entry.name.as_str()),
        upper,
        red,
        seen,
    )
}

fn cfb_name_cmp(ctx: &DecodeContext<'_>, left: &str, right: &str) -> Result<Ordering, CodecError> {
    let left_len = ctx.admit_iter(left, "compare CFB sibling names")?.encode_utf16().count();
    let right_len = ctx.admit_iter(right, "compare CFB sibling names")?.encode_utf16().count();
    let length_order = left_len.cmp(&right_len);
    if length_order != Ordering::Equal { return Ok(length_order); }
    let left_units = ctx.admit_iter(left, "compare CFB sibling names")?.encode_utf16().map(cfb_upper_unit);
    let right_units = ctx.admit_iter(right, "compare CFB sibling names")?.encode_utf16().map(cfb_upper_unit);
    for (left, right) in left_units.zip(right_units) {
        let order = left.cmp(&right);
        if order != Ordering::Equal { return Ok(order); }
    }
    Ok(Ordering::Equal)
}

fn path_key(ctx: &DecodeContext<'_>, path: &str) -> Result<Vec<Vec<u16>>, CodecError> {
    let mut components = Vec::new();
    let mut component = Vec::new();
    for character in ctx.admit_iter(path, "scan CFB path key")? {
        if character == '/' {
            ctx.push_vec(&mut components, std::mem::take(&mut component), "CFB path key components")?;
        } else {
            let mut encoded = [0_u16; 2];
            for &unit in ctx.admit_iter(character.encode_utf16(&mut encoded), "encode CFB path units")? {
                ctx.push_vec(&mut component, cfb_upper_unit(unit), "CFB path key units")?;
            }
        }
    }
    ctx.push_vec(&mut components, component, "CFB path key components")?;
    Ok(components)
}

fn cfb_upper_unit(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut uppercase = character.to_uppercase();
    match (uppercase.next(), uppercase.next()) {
        (Some(first), None) if first.len_utf16() == 1 => {
            let mut encoded = [0u16; 2];
            first.encode_utf16(&mut encoded)[0]
        }
        _ => unit,
    }
}

fn range_lock_sector(version: CompoundVersion, file_size: u64) -> Option<u32> {
    if version != CompoundVersion::V4 || file_size <= RANGE_LOCK_END {
        return None;
    }
    // V4 fixes the sector size at 4096 bytes; this address fits a u32 sector id.
    u32::try_from(
        RANGE_LOCK_START / cadmpeg_core::decode::u64_from_index(version.sector_size()) - 1,
    )
    .ok()
}

#[derive(Clone, Copy)]
enum ChainLength {
    Declared(NonZeroUsize),
    Unbounded,
}

#[derive(Clone, Copy)]
enum ChainRole {
    Directory,
    MiniFat,
    RootMiniStream,
    Stream,
    MiniStream,
}

impl ChainRole {
    fn name(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::MiniFat => "mini FAT",
            Self::RootMiniStream => "root mini stream",
            Self::Stream => "stream",
            Self::MiniStream => "mini stream",
        }
    }
}

fn chain(
    ctx: &DecodeContext<'_>,
    fat: &[u32],
    sector_count: usize,
    start: u32,
    length: Option<ChainLength>,
    role: ChainRole,
) -> Result<Option<SectorChain>, CodecError> {
    let accepts_free = matches!(
        role,
        ChainRole::RootMiniStream | ChainRole::Stream | ChainRole::MiniStream
    );
    let role = role.name();
    let expected = match length {
        None => {
            if start == END_OF_CHAIN || (accepts_free && start == FREE_SECTOR) {
                return Ok(None);
            }
            return malformed(ctx.format_retained(format_args!("empty CFB {role} has an invalid start sector"), "CFB sector chain error")?);
        }
        Some(ChainLength::Declared(count)) => Some(count),
        Some(ChainLength::Unbounded) => None,
    };
    let limit = expected.map_or(sector_count, NonZeroUsize::get);
    if let Some(count) = expected {
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(count.get()),
            "retain CFB sector chain",
        )?;
    }
    let mut traversal_scratch = ctx.reserve_scoped(0, "walk CFB sector chain")?;
    if start == END_OF_CHAIN {
        return if expected.is_some() {
            malformed(ctx.format_retained(format_args!("CFB {role} chain length does not match its declaration"), "CFB sector chain error")?)
        } else {
            malformed(ctx.format_retained(format_args!("empty CFB {role}"), "CFB sector chain error")?)
        };
    }
    let mut output = SectorChain {
        first: start,
        rest: ctx.vector_storage(
            expected.map_or(0, |count| count.get() - 1),
            "retain CFB sector chain",
        )?,
    };
    let mut seen = BTreeSet::new();
    let mut current = start;
    while current != END_OF_CHAIN {
        ctx.charge_work(1, "scan CFB sector chain")?;
        if cadmpeg_core::decode::index_from_u32(current) >= sector_count || seen.len() == limit {
            return malformed(ctx.format_retained(format_args!("CFB {role} chain is cyclic, overlong, or out of range"), "CFB sector chain error")?);
        }
        if !ctx.insert_scoped_btree_set(
            &mut traversal_scratch,
            &mut seen,
            current,
            "compare CFB visited sectors",
            "walk CFB sector chain",
        )? {
            return malformed(ctx.format_retained(format_args!("CFB {role} chain is cyclic, overlong, or out of range"), "CFB sector chain error")?);
        }
        if expected.is_none() {
            if current == start {
                ctx.charge_collection_items(1, "retain CFB sector chain")?;
            } else {
                ctx.push_vec(&mut output.rest, current, "retain CFB sector chain")?;
            }
        } else if current != start {
            ctx.reserve_capacity(&mut output.rest, 1, "CFB sector chain slot")?;
            output.rest.push(current);
        }
        current = *fat
            .get(cadmpeg_core::decode::index_from_u32(current))
            .ok_or_else(|| CodecError::malformed(format_args!("CFB {role} FAT link is absent")))?;
        if matches!(current, FREE_SECTOR | FAT_SECTOR | DIFAT_SECTOR) {
            return malformed(ctx.format_retained(format_args!("CFB {role} chain enters a reserved sector role"), "CFB sector chain error")?);
        }
    }
    if expected.is_some_and(|count| output.len() != count.get()) {
        return malformed(ctx.format_retained(format_args!("CFB {role} chain length does not match its declaration"), "CFB sector chain error")?);
    }
    Ok(Some(output))
}

fn join_sectors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    sector_size: usize,
    sector_count: usize,
    sectors: Option<(&u32, &[u32])>,
) -> Result<Vec<u8>, CodecError> {
    let (first, rest) = sectors.map_or((None, &[][..]), |(first, rest)| (Some(first), rest));
    let length = rest.len()
        .checked_add(usize::from(first.is_some()))
        .and_then(|count| count.checked_mul(sector_size))
        .ok_or_else(|| ctx.refuse_codec_limit("join CFB sectors", u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(length),
        "join CFB sectors",
    )?;
    let mut output = ctx.vector_storage(length, "join CFB sectors")?;
    for &sector in first.into_iter().chain(ctx.admit_iter(rest, "walk CFB joined sectors")?) {
        let data = sector_slice(bytes, sector_size, sector_count, sector)
            .ok_or_else(|| CodecError::Malformed("CFB sector is absent".into()))?;
        if data.len() != sector_size {
            return malformed("CFB structural sector is truncated");
        }
        ctx.reserve_capacity(&mut output, data.len(), "join CFB sector slots")?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(data.len()), "copy CFB sectors")?;
        output.extend_from_slice(data);
    }
    Ok(output)
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
fn le_u16_array(bytes: [u8; 2]) -> u16 {
    cadmpeg_core::bytes::assemble_u16_le(bytes)
}
fn le_u32_array(bytes: [u8; 4]) -> u32 {
    cadmpeg_core::bytes::assemble_u32_le(bytes)
}
fn le_u64_array(bytes: [u8; 8]) -> u64 {
    cadmpeg_core::bytes::assemble_u64_le(bytes)
}
fn malformed<T>(message: impl Into<String>) -> Result<T, CodecError> {
    Err(CodecError::Malformed(message.into()))
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::io::{self, Read};

    use cadmpeg_core::container::ContainerRole;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use cadmpeg_test_support::bytes::{put_u16, put_u32};

    use super::{
        cfb_name_cmp, cfb_upper_unit, chain, parse_directory, path_key, range_lock_sector,
        read_detection_prefix, validate_sibling_tree, ChainLength, ChainRole, CompoundEntry,
        CompoundPrefixProbe, CompoundSnapshot, CompoundState, CompoundVersion, DirectorySlot,
        DIFAT_SECTOR, END_OF_CHAIN, FAT_SECTOR, FREE_SECTOR, MAGIC, NO_STREAM, RANGE_LOCK_END,
    };

    const SECTOR_SIZE: usize = 512;

    fn with_context<T>(
        bytes: &[u8],
        policy: &DecodePolicy,
        use_context: impl FnOnce(&DecodeContext<'_>) -> T,
    ) -> T {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy)
            .expect("fixture fits service profile");
        use_context(&ctx)
    }

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

    fn probe(bytes: &[u8]) -> CompoundPrefixProbe {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("probe root");
        let (probe, storage) =
            CompoundPrefixProbe::inspect_with_context(&ctx, root).expect("probe");
        drop(storage);
        probe
    }

    #[test]
    fn sector_chain_error_refuses_before_formatting() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = chain(&ctx, &[], 0, 0, None, ChainRole::Directory).expect_err("error format work") else { panic!("refusal") };
        assert_eq!(first.operation, "CFB sector chain error");
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(1, "later").expect_err("fused refusal") else { panic!("refusal") };
        assert_eq!(first, repeated);
    }

    #[test]
    fn directory_record_visits_refuse_before_decoding() {
        let bytes = [0u8; 128];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = super::parse_directory(&ctx, &bytes, super::CompoundVersion::V3).expect_err("record visits") else { panic!("refusal") };
        assert_eq!(first.operation, "visit CFB directory records");
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(1, "later").expect_err("fused refusal") else { panic!("refusal") };
        assert_eq!(first, repeated);
    }

    #[test]
    fn joined_sectors_use_bounded_parts_and_refuse_before_copy() {
        let bytes = [0xabu8; 1536];
        let first = 1;
        let rest = [0];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(super::join_sectors(&ctx, &bytes, 512, 2, Some((&first, &rest))).expect("joined sectors"), [0xabu8; 1024]);
        assert!(super::join_sectors(&ctx, &bytes, 512, 2, None).expect("empty chain").is_empty());
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = super::join_sectors(&ctx, &bytes, 512, 2, Some((&first, &[]))).expect_err("copy work") else { panic!("refusal") };
        assert_eq!(first.operation, "copy CFB sectors");
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(1, "later").expect_err("fused refusal") else { panic!("refusal") };
        assert_eq!(first, repeated);
    }

    #[test]
    fn sibling_tree_refuses_depth_before_descending_black_chain() {
        let mut directory = [0_u8; 384];
        directory_entry(
            &mut directory,
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
            &mut directory,
            1,
            "A",
            2,
            NO_STREAM,
            2,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        directory_entry(
            &mut directory,
            2,
            "B",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        let entries = with_context(&directory, &DecodePolicy::service(), |ctx| {
            parse_directory(ctx, &directory, CompoundVersion::V3).expect("directory parses")
        });
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let error = with_context(&[], &policy, |ctx| validate_sibling_tree(ctx, &entries, 1))
            .expect_err("second black node exceeds active depth");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth));
    }

    #[test]
    fn nested_storages_refuse_the_callers_depth_limit() {
        let file = fixture();
        let mut state = with_context(&file, &DecodePolicy::service(), |ctx| {
            CompoundState::parse(ctx, &file).expect("allocation tables parse")
        });
        let mut directory = [0_u8; 384];
        directory_entry(
            &mut directory,
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
            &mut directory,
            1,
            "A",
            1,
            NO_STREAM,
            NO_STREAM,
            2,
            END_OF_CHAIN,
            0,
        );
        directory_entry(
            &mut directory,
            2,
            "B",
            1,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        state.directory = with_context(&directory, &DecodePolicy::service(), |ctx| {
            parse_directory(ctx, &directory, CompoundVersion::V3).expect("directory parses")
        });
        with_context(&[], &DecodePolicy::service(), |ctx| {
            assert_eq!(
                state
                    .build_entries(ctx, 1)
                    .expect("nested storages parse")
                    .len(),
                2
            );
        });
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        let error = with_context(&[], &policy, |ctx| state.build_entries(ctx, 1))
            .expect_err("nested storage traversal exceeds active depth");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth));
    }

    #[test]
    fn sector_chain_admits_visited_storage_items_and_work_before_traversal() {
        use cadmpeg_core::decode::ResourceDimension;
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 1,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test selects traversal resources"),
            }
            let error = with_context(&[], &policy, |ctx| {
                chain(
                    ctx,
                    &[END_OF_CHAIN],
                    1,
                    0,
                    Some(ChainLength::Declared(
                        std::num::NonZeroUsize::new(1).expect("one sector"),
                    )),
                    ChainRole::Stream,
                )
            })
            .expect_err("visited traversal exceeds caller allowance");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension)
            );
        }
    }

    #[test]
    fn stream_open_admits_only_needed_sector_view_storage() {
        use cadmpeg_core::decode::ResourceDimension;
        for fragmented in [false, true] {
            let mut file = fixture();
            if fragmented {
                let fat = sector_mut(&mut file, 11);
                put_u32(fat, 2 * 4, 4);
                put_u32(fat, 4 * 4, 3);
                put_u32(fat, 3 * 4, 5);
            }
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
            let snapshot = CompoundSnapshot::new(&ctx, root).expect("valid allocation");
            let stream = snapshot.stream(&ctx, "Store/Large").expect("lookup admission").expect("large stream");
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 0;
            with_context(&[], &policy, |ctx| {
                let opened = snapshot.open(ctx, stream);
                if fragmented {
                    assert!(
                        matches!(opened, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
                    );
                } else {
                    assert_eq!(
                        opened.expect("contiguous stream borrows sectors").window(),
                        &[0x5a; 4096]
                    );
                }
            });
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let error = with_context(&[], &policy, |ctx| {
                snapshot
                    .open(ctx, stream)
                    .expect_err("sector scan is admitted")
            });
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
            );
        }
    }

    #[test]
    fn storage_parent_path_copy_holds_scoped_bytes() {
        let file = fixture();
        let mut state = with_context(&file, &DecodePolicy::service(), |ctx| {
            CompoundState::parse(ctx, &file).expect("allocation tables")
        });
        let mut directory = [0_u8; 384];
        let name = "A".repeat(31);
        directory_entry(
            &mut directory,
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
            &mut directory,
            1,
            &name,
            1,
            NO_STREAM,
            NO_STREAM,
            2,
            END_OF_CHAIN,
            0,
        );
        directory_entry(
            &mut directory,
            2,
            "B",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        state.directory = with_context(&directory, &DecodePolicy::service(), |ctx| {
            parse_directory(ctx, &directory, CompoundVersion::V3).expect("directory")
        });
        let entries = with_context(&[], &DecodePolicy::service(), |ctx| {
            state.build_entries(ctx, 1).expect("entries")
        });
        assert_eq!(entries[1].path(), format!("{name}/B"));
        let mut policy = DecodePolicy::service();
        // One reached-node bound and one pending u32 precede the parent copy.
        let node = 11 * std::mem::size_of::<u32>() + 18 * std::mem::size_of::<usize>();
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
            node + std::mem::size_of::<u32>() + name.len() - 1,
        );
        let error = with_context(&[], &policy, |ctx| state.build_entries(ctx, 1))
            .expect_err("parent copy must stay admitted through child traversal");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "hold CFB storage parent path")
        );
    }

    #[test]
    fn sector_ownership_validation_uses_the_callers_resources() {
        use cadmpeg_core::decode::ResourceDimension;
        let file = fixture();
        let state = with_context(&file, &DecodePolicy::service(), |ctx| {
            CompoundState::parse(ctx, &file).expect("allocation tables parse")
        });
        let entries = with_context(&[], &DecodePolicy::service(), |ctx| {
            state.build_entries(ctx, 1).expect("directory entries")
        });
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test selects ownership resources"),
            }
            let error = with_context(&[], &policy, |ctx| {
                state.validate_sector_ownership(ctx, &entries)
            })
            .expect_err("ownership pass must admit its own resources");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension)
            );
        }
    }

    #[test]
    fn detection_prefix_never_reads_past_its_byte_limit() {
        let mut source = CountingReader {
            inner: &[b'x'; 1024],
            bytes_read: 0,
        };

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_input_bytes = 16;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let prefix = read_detection_prefix(&ctx, &mut source, 128 * 1024)
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
                .open(&ctx, snapshot.stream(&ctx, "small").expect("lookup admission").expect("small stream exists"),)
                .expect("small stream opens")
                .window(),
            b"small"
        );
        assert_eq!(
            snapshot
                .open(
                    &ctx,
                    snapshot
                        .stream(&ctx, "Store/Large").expect("lookup admission")
                        .expect("regular stream exists"),
                )
                .expect("regular stream opens")
                .window(),
            vec![0x5a; 4096]
        );
        assert!(matches!(
            snapshot.entry(&ctx, "STORE").expect("lookup admission"),
            Some(CompoundEntry::Storage(_))
        ));
    }

    #[test]
    fn stream_open_uses_absolute_coordinates_for_nonzero_root_views() {
        for prefix_len in [16, 1024] {
            for empty in [false, true] {
                let mut file = fixture();
                if empty {
                    directory_entry(
                        sector_mut(&mut file, 0),
                        1,
                        "Small",
                        2,
                        NO_STREAM,
                        2,
                        NO_STREAM,
                        END_OF_CHAIN,
                        0,
                    );
                    put_u32(sector_mut(&mut file, 10), 0, FREE_SECTOR);
                }
                let mut prefixed = vec![0_u8; prefix_len];
                prefixed.extend_from_slice(&file);
                let arena = DecodeArena::new();
                let (ctx, root) =
                    DecodeContext::from_root_bytes(&prefixed, &arena, &DecodePolicy::service())
                        .expect("prefixed fixture fits policy");
                let cfb = root
                    .child(prefix_len, prefixed.len())
                    .expect("CFB child view");
                let snapshot =
                    CompoundSnapshot::new(&ctx, cfb).expect("CFB parses within child view");
                let small = snapshot
                    .open(&ctx, snapshot.stream(&ctx, "Small").expect("lookup admission").expect("small stream"))
                    .expect("mini or empty stream opens within child view");
                assert_eq!(small.window(), if empty { &b""[..] } else { &b"small"[..] });
                assert_eq!(
                    small.start(),
                    if empty {
                        0
                    } else {
                        prefix_len + 2 * SECTOR_SIZE
                    }
                );
                assert_eq!(
                    snapshot
                        .regular_sector_view(2)
                        .expect("regular sector view")
                        .start(),
                    prefix_len + 3 * SECTOR_SIZE
                );
                let large = snapshot
                    .open(
                        &ctx,
                        snapshot.stream(&ctx, "Store/Large").expect("lookup admission").expect("regular stream"),
                    )
                    .expect("regular stream opens within child view");
                assert_eq!(large.window(), &[0x5a; 4096]);
            }
        }
    }

    #[test]
    fn compound_summary_refuses_before_classification() {
        let file = fixture();
        let arena = DecodeArena::new();
        let (setup, root) = DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("setup");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let called = std::cell::Cell::new(false);
        assert!(matches!(snapshot.container_entries(&ctx, |_| { called.set(true); ContainerRole::Stream }), Err(CodecError::ResourceLimit(limit)) if limit.operation == "CFB container summaries"));
        assert!(!called.get());
    }

    #[test]
    fn empty_streams_preserve_both_admitted_start_markers() {
        for marker in [END_OF_CHAIN, FREE_SECTOR] {
            let mut file = fixture();
            directory_entry(
                sector_mut(&mut file, 0),
                1,
                "Small",
                2,
                NO_STREAM,
                2,
                NO_STREAM,
                marker,
                0,
            );
            put_u32(sector_mut(&mut file, 10), 0, FREE_SECTOR);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::default();
            let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
                .expect("empty stream fixture fits policy");
            let snapshot = CompoundSnapshot::new(&ctx, root).expect("empty stream parses");
            let stream = snapshot.stream(&ctx, "Small").expect("lookup admission").expect("empty stream exists");
            assert_eq!(stream.start_sector(), marker);
            assert_eq!(stream.logical_size(), 0);
            assert!(snapshot
                .open(&ctx, stream)
                .expect("empty stream opens")
                .window()
                .is_empty());
            let summary = snapshot.container_entries(&ctx, |_| ContainerRole::Stream).expect("summary admission");
            let entry = summary
                .iter()
                .find(|entry| entry.name == "Small")
                .expect("stream summary");
            assert_eq!(entry.attributes["start_sector"], marker.to_string());
        }
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
                    .stream(&ctx, "Store/Large").expect("lookup admission")
                    .expect("regular stream exists"),
            )
            .expect("regular stream opens through the partial sector");
        assert_eq!(stream.window().len(), 4110);
        assert!(stream.window().iter().all(|byte| *byte == 0x5a));

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
                    .stream(&ctx, "Store/Large").expect("lookup admission")
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
        let foreign = second.stream(&ctx, "Small").expect("lookup admission").expect("foreign stream exists");
        assert!(first.open(&ctx, foreign).is_err());

        let owned = first.stream(&ctx, "Small").expect("lookup admission").expect("owned stream exists").clone();
        assert_eq!(
            first
                .open(&ctx, &owned)
                .expect("owned clone opens")
                .window(),
            b"small"
        );
    }

    #[test]
    fn fat_id_population_is_refused_before_exceeding_its_declaration() {
        let mut file = fixture();
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, 11);
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let error = with_context(&file, &policy, |ctx| CompoundState::parse(ctx, &file))
            .expect_err("second FAT id exceeds the one admitted slot");
        assert!(matches!(error, CodecError::Malformed(detail)
            if detail == "CFB FAT ids exceed the declared count"));
        assert!(
            matches!(probe(&file), CompoundPrefixProbe::Malformed(detail)
            if detail == "CFB FAT ids exceed the declared count")
        );
    }

    #[test]
    fn prefix_probe_reaches_directory_names_without_scanning_bytes() {
        let file = fixture();
        let CompoundPrefixProbe::DirectoryEvidence(paths) = probe(&file) else {
            panic!("usable prefix")
        };
        assert!(paths.iter().any(|path| path == "Store/Large"));
        assert_eq!(probe(b"not cfb"), CompoundPrefixProbe::NotCompound);
        assert_eq!(probe(&file[..400]), CompoundPrefixProbe::Incomplete);
    }

    #[test]
    fn prefix_probe_propagates_directory_budget_refusal() {
        let file = fixture();
        assert!(matches!(
            probe(&file),
            CompoundPrefixProbe::DirectoryEvidence(_)
        ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("fixture fits input limit");
        assert!(matches!(
            CompoundPrefixProbe::inspect_with_context(&ctx, cadmpeg_core::decode::View::over_retained(&file)),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn prefix_probe_uses_default_fat_count_limit() {
        let mut file = fixture();
        assert!(matches!(
            probe(&file),
            CompoundPrefixProbe::DirectoryEvidence(_)
        ));
        let limit = DecodePolicy::default().limits.max_collection_items;
        let count = u32::try_from(limit + 1).expect("default limit fits FAT count");
        put_u32(&mut file, 44, count);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::default()).expect("root");
        assert!(matches!(
            CompoundPrefixProbe::inspect_with_context(&ctx, root),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && refusal.limit == limit && refusal.used == 0 && refusal.additional == u64::from(count)
        ));
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
            probe(&file),
            CompoundPrefixProbe::DirectoryEvidence(_)
        ));
        assert_eq!(
            probe(&file[..SECTOR_SIZE * 13]),
            CompoundPrefixProbe::Incomplete
        );
    }

    #[test]
    fn prefix_probe_uses_available_leading_fat_coverage() {
        let mut prefix = fixture();
        put_u32(&mut prefix, 44, 2);
        put_u32(&mut prefix, 80, 1_000);
        assert!(matches!(
            probe(&prefix),
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
        assert!(snapshot.stream(&ctx, "Store/Large").expect("lookup admission").is_some());
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
                .open(&ctx, snapshot.stream(&ctx, "Wide").expect("lookup admission").expect("stream exists"))
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
            with_context(&directory, &DecodePolicy::service(), |ctx| parse_directory(
                ctx,
                &directory,
                CompoundVersion::V4
            ))
            .expect("v4 directory parses")[0]
                .live()
                .expect("live root")
                .size,
            0x1_0000_0001
        );
        assert_eq!(
            with_context(&directory, &DecodePolicy::service(), |ctx| parse_directory(
                ctx,
                &directory,
                CompoundVersion::V3
            ))
            .expect("v3 directory parses")[0]
                .live()
                .expect("live root")
                .size,
            1
        );
    }

    #[test]
    fn v4_requires_zero_header_padding() {
        let mut file = fixture_v4();
        file[512] = 1;
        assert!(matches!(probe(&file), CompoundPrefixProbe::Malformed(_)));
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
            assert!(matches!(probe(&file), CompoundPrefixProbe::Malformed(_)));
        }

        let mut file = fixture();
        file[34] = 1;
        assert!(matches!(probe(&file), CompoundPrefixProbe::Malformed(_)));
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
            range_lock_sector(CompoundVersion::V4, RANGE_LOCK_END + 4096),
            Some(0x0007_fffe)
        );
        assert_eq!(range_lock_sector(CompoundVersion::V4, RANGE_LOCK_END), None);
        assert_eq!(
            range_lock_sector(CompoundVersion::V3, RANGE_LOCK_END + 512),
            None
        );
    }

    #[test]
    fn compound_path_lookup_refuses_before_key_work_or_storage() {
        let bytes = fixture();
        let arena = DecodeArena::new();
        let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("setup");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let CodecError::ResourceLimit(first) = snapshot.entry(&ctx, "Store").expect_err("lookup work") else { panic!("resource refusal") };
        assert_eq!(first.operation, "scan CFB path key");
        let CodecError::ResourceLimit(repeated) = snapshot.stream(&ctx, "Small").expect_err("fused lookup") else { panic!("resource refusal") };
        assert_eq!(first, repeated);
        policy.limits.max_work_units = DecodePolicy::service().limits.max_work_units;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(snapshot.entry(&ctx, "Store"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn directory_name_queries_refuse_before_scans() {
        let file = fixture();
        let arena = DecodeArena::new();
        let (setup, _) = DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("setup");
        let directory = parse_directory(&setup, &file[512..1024], CompoundVersion::V3).expect("directory");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = cfb_name_cmp(&ctx, "alpha", "ALPHA").expect_err("comparison work") else { panic!("refusal") };
        let CodecError::ResourceLimit(repeated) = super::DirectoryName::new(&ctx, "alpha".into()).expect_err("fused name work") else { panic!("refusal") };
        assert_eq!(first, repeated);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(super::validate_root(&ctx, &directory), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan CFB root entries"));
    }

    #[test]
    fn directory_keys_use_length_preserving_simple_uppercase_units() {
        with_context(&[], &DecodePolicy::service(), |ctx| {
            assert_eq!(cfb_name_cmp(ctx, "alpha", "ALPHA").expect("comparison"), Ordering::Equal);
            assert_eq!(path_key(ctx, "Store/alpha").expect("key"), path_key(ctx, "store/ALPHA").expect("key"));
            assert_ne!(path_key(ctx, "ß").expect("key"), path_key(ctx, "SS").expect("key"));
        });
        assert_eq!(cfb_upper_unit(0xd800), 0xd800);
    }

    #[test]
    fn directory_utf16_name_charges_exact_retained_utf8_bytes() {
        let mut directory = [0_u8; 128];
        directory_entry(
            &mut directory,
            0,
            "ࠀ",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        let slot_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<DirectorySlot>());
        for limit in [slot_bytes + 2, slot_bytes + 3] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let result = with_context(&directory, &policy, |ctx| {
                parse_directory(ctx, &directory, CompoundVersion::V3)
            });
            if limit == slot_bytes + 2 {
                assert!(matches!(result, Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        && refusal.used == slot_bytes && refusal.additional == 3));
            } else {
                assert_eq!(
                    result.expect("exact name budget")[0]
                        .live()
                        .expect("live root directory slot")
                        .name
                        .as_str(),
                    "ࠀ"
                );
            }
        }
    }

    #[test]
    fn accepts_stale_unallocated_directory_fields() {
        let mut directory = vec![0_u8; 128];
        directory[68..80].fill(0xff);
        directory[8] = 1;
        let entries = with_context(&directory, &DecodePolicy::service(), |ctx| {
            parse_directory(ctx, &directory, CompoundVersion::V3)
        })
        .expect("unallocated slot is skipped");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], DirectorySlot::Free));
    }

    #[test]
    fn empty_live_directory_names_are_refused_before_path_construction() {
        for kind in [1, 2] {
            let mut directory = [0_u8; 384];
            directory_entry(
                &mut directory,
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
                &mut directory,
                1,
                "",
                kind,
                NO_STREAM,
                NO_STREAM,
                if kind == 1 { 2 } else { NO_STREAM },
                END_OF_CHAIN,
                0,
            );
            directory_entry(
                &mut directory,
                2,
                "Child",
                2,
                NO_STREAM,
                NO_STREAM,
                NO_STREAM,
                END_OF_CHAIN,
                0,
            );
            let error = with_context(&directory, &DecodePolicy::service(), |ctx| {
                parse_directory(ctx, &directory, CompoundVersion::V3)
            })
            .expect_err("empty live name has no path representation");
            assert!(matches!(error, CodecError::NotImplemented(message)
                if message == "CFB empty live directory names cannot be represented as paths"));
        }
        assert!(with_context(&[], &DecodePolicy::service(), |ctx| super::DirectoryName::new(ctx, String::new())).is_err());
        assert_eq!(
            with_context(&[], &DecodePolicy::service(), |ctx| super::DirectoryName::new(ctx, " ".into()))
                .expect("a space is a valid live name")
                .as_str(),
            " "
        );
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
                .stream(&ctx, "Small").expect("lookup admission")
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

    fn fixture() -> Vec<u8> {
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
            put_u32(
                fat,
                id * 4,
                u32::try_from(id + 1).expect("test FAT index fits u32"),
            );
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
        put_u16(
            entry,
            64,
            u16::try_from((encoded.len() + 1) * 2).expect("test name length fits u16"),
        );
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
}
