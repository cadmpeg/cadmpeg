// SPDX-License-Identifier: Apache-2.0
//! Lazy, budgeted Microsoft Compound File Binary (CFB) snapshots.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use cadmpeg_core::decode::{ByteRange, DecodeContext, ResourceLimit, ScopedReservation, View};
use cadmpeg_core::{CodecError, ContainerEntry};

use crate::layout::directory_entry as directory_layout;

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const DIFAT_SECTOR: u32 = 0xffff_fffc;
// NO_STREAM and FREE_SECTOR are the CFB specification names for the same value.
const NO_STREAM: u32 = FREE_SECTOR;
const V3_MAX_FILE_SIZE: u64 = 0x8000_0000;
const V3_MAX_STREAM_SIZE: u64 = 0x8000_0000;
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
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
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
struct SectorChain {
    first: u32,
    rest: Vec<u32>,
}

impl SectorChain {
    fn len(&self) -> usize {
        1 + self.rest.len()
    }

    fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        mut visit: impl FnMut(u32) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visit(self.first)?;
        ctx.fold(&self.rest, (), |(), &sector| visit(sector), "visit CFB chain sectors")
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
    Empty(u32),
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
            StreamData::Empty(start) => *start,
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
        // The fixed field and counted terminator already bound strict
        // UTF-16 content to 31 units before this decoded name is constructed.
        if ctx.any_by(
            name.chars(),
            |character| Ok(matches!(character, '/' | '\\' | ':' | '!')),
            "check CFB directory name characters",
        )?
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
pub struct CompoundSnapshot<'a, 'ctx> {
    root: View<'a>,
    snapshot_id: u64,
    version: CompoundVersion,
    sector_count: usize,
    root_mini_chain: Option<SectorChain>,
    entries: Vec<CompoundEntry>,
    by_path: BTreeMap<Vec<Vec<u16>>, usize>,
    streams_by_id: BTreeMap<CompoundStreamId, usize>,
    _metadata_storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> CompoundSnapshot<'a, 'ctx> {
    /// Parses and validates the complete CFB structure without opening streams.
    /// Stream extents are checked against their available bytes when opened.
    pub fn new(ctx: &'ctx DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let mut construction_storage = ctx.reserve_scoped(0, "CFB construction state")?;
        let mut root_chain_storage = ctx.reserve_scoped(0, "CFB root mini-chain backing")?;
        let parsed = CompoundState::parse(
            ctx, root.window(), &mut construction_storage, &mut root_chain_storage,
        )?;
        let mut metadata_storage = ctx.reserve_scoped(0, "CFB snapshot metadata")?;
        let snapshot_id = NEXT_COMPOUND_SNAPSHOT_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let entries = parsed.build_entries(ctx, snapshot_id, &mut metadata_storage)?;
        parsed.validate_sector_ownership(ctx, &entries)?;
        let CompoundState {
            version, sector_count, fat, mini_fat, directory, directory_chain,
            mini_fat_chain, root_mini_chain, fat_sectors, difat_sectors,
            range_lock_sector: _,
        } = parsed;
        drop((fat, mini_fat, directory, directory_chain, mini_fat_chain, fat_sectors, difat_sectors));
        drop(construction_storage);
        metadata_storage.absorb(&mut root_chain_storage)?;
        let mut by_path = BTreeMap::new();
        let mut streams_by_id = BTreeMap::new();

        ctx.fold(&entries, 0_usize, |index, entry| {
            let key = metadata_storage.with_storage(|| path_key(ctx, entry.path()))?;
            // Strict sibling ordering and unique reached directory ids already
            // prove unique nonempty, separator-free path components.
            metadata_storage.with_storage(|| {
                ctx.insert_btree_map(&mut by_path, key, index, "index CFB path")
            })?;
            if let CompoundEntry::Stream(stream) = entry {
                metadata_storage.with_storage(|| {
                    ctx.insert_btree_map(&mut streams_by_id, stream.id(), index, "index CFB stream")
                })?;
            }
            Ok(index + 1)
        }, "visit CFB indexed entries")?;
        Ok(Self {
            root, snapshot_id, version, sector_count, root_mini_chain, entries,
            by_path, streams_by_id, _metadata_storage: metadata_storage,
        })
    }

    /// Returns the CFB major version.
    pub const fn major_version(&self) -> u16 {
        self.version.major()
    }

    /// Returns the regular-sector size.
    pub const fn sector_size(&self) -> usize {
        self.version.sector_size()
    }

    /// Returns entries in stable directory traversal order.
    pub fn entries(&self) -> &[CompoundEntry] {
        &self.entries
    }

    /// Finds an entry by a case-insensitive CFB path key.
    pub fn entry(
        &self,
        ctx: &DecodeContext<'_>,
        path: &str,
    ) -> Result<Option<&CompoundEntry>, CodecError> {
        let (entry, storage) =
            ctx.with_scoped_storage("CFB path lookup key", || -> Result<_, CodecError> {
                let key = path_key(ctx, path)?;
                Ok(ctx
                    .get_btree_map(&self.by_path, &key, "CFB path lookup")?
                    .map(|index| &self.entries[*index]))
            })?;
        drop(storage);
        Ok(entry)
    }

    /// Finds a stream by a case-insensitive CFB path key.
    pub fn stream(
        &self,
        ctx: &DecodeContext<'_>,
        path: &str,
    ) -> Result<Option<&CompoundStreamEntry>, CodecError> {
        Ok(match self.entry(ctx, path)? {
            Some(CompoundEntry::Stream(entry)) => Some(entry),
            Some(CompoundEntry::Storage(_)) | None => None,
        })
    }

    /// Finds a stream by stable directory identity.
    pub fn stream_by_id(
        &self,
        ctx: &DecodeContext<'_>,
        id: CompoundStreamId,
    ) -> Result<Option<&CompoundStreamEntry>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.streams_by_id, &id, "CFB stream id lookup")?
            .and_then(|index| match &self.entries[*index] {
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
        let first = sector_view(chain.first)?;
        let (end, available_bytes, contiguous) = ctx.fold(
            &chain.rest, (first.end(), first.window().len(), true),
            |(end, available_bytes, contiguous), &sector| {
                let view = sector_view(sector)?;
                let available_bytes = available_bytes.checked_add(view.window().len()).ok_or_else(|| {
                    CodecError::Malformed("CFB available stream byte length overflow".into())
                })?;
                Ok((view.end(), available_bytes, contiguous && view.start() == end))
            },
            "visit CFB stream sectors",
        )?;
        if logical_size > available_bytes {
            return malformed(ctx.format_retained(
                format_args!("CFB stream {} is shorter than declared", entry.path),
                "CFB stream extent error",
            )?);
        }
        let opened = if contiguous {
            self.root.child(first.start(), end).ok_or_else(|| {
                CodecError::Malformed("CFB contiguous stream range escapes input".into())
            })?
        } else {
            let mut views = ctx.temporary_vec(chain.len(), "CFB stream sector views")?;
            ctx.reserve_capacity(&mut views.0, 1, "CFB stream view slot")?;
            let first_len = logical_size.min(first.window().len());
            let first = first.child(first.start(), first.start() + first_len).ok_or_else(|| {
                CodecError::Malformed("CFB logical first-sector range escapes input".into())
            })?;
            views.0.push(first);
            ctx.fold(&chain.rest, logical_size - first_len, |remaining, &sector| {
                let view = sector_view(sector)?;
                let payload_len = remaining.min(view.window().len());
                let payload = view.child(view.start(), view.start() + payload_len).ok_or_else(|| {
                    CodecError::Malformed("CFB logical sector range escapes input".into())
                })?;
                ctx.reserve_capacity(&mut views.0, 1, "CFB stream view slot")?;
                views.0.push(payload);
                Ok(remaining - payload_len)
            }, "visit CFB stream sectors")?;
            ctx.concat_views(&views.0)?
        };
        let logical_end = opened
            .start()
            .checked_add(logical_size)
            .ok_or_else(|| CodecError::Malformed("CFB logical stream end overflow".into()))?;
        opened.child(opened.start(), logical_end).ok_or_else(|| {
            CodecError::Malformed("CFB logical stream range escapes available bytes".into())
        })
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
                ctx.format_retained(
                    format_args!("{}", entry.directory_id()),
                    "CFB summary attribute value",
                )?,
                "CFB summary attributes",
            )?;
            let storage = match entry {
                CompoundEntry::Storage(_) => EntryStorage::Directory,
                CompoundEntry::Stream(stream) => {
                    if let Some(allocation) = stream.allocation() {
                        ctx.insert_btree_map(
                            &mut attributes,
                            ctx.copy_retained_text("allocation", "CFB summary attribute key")?,
                            ctx.copy_retained_text(
                                allocation.label(),
                                "CFB summary attribute value",
                            )?,
                            "CFB summary attributes",
                        )?;
                    }
                    ctx.insert_btree_map(
                        &mut attributes,
                        ctx.copy_retained_text("start_sector", "CFB summary attribute key")?,
                        ctx.format_retained(
                            format_args!("{}", stream.start_sector()),
                            "CFB summary attribute value",
                        )?,
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
            self.version.sector_size(),
            self.sector_count,
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
        let regular_ordinal = offset / self.version.sector_size();
        let within = offset % self.version.sector_size();
        let &regular_sector = self
            .root_mini_chain
            .as_ref()
            .and_then(|chain| chain.get(regular_ordinal))
            .ok_or_else(|| {
                CodecError::Malformed("CFB mini sector escapes the root mini stream".into())
            })?;
        let sector = self.regular_sector_view(regular_sector)?;
        if within >= sector.window().len() {
            return malformed("CFB mini sector crosses a regular-sector boundary");
        }
        let start = sector.start().checked_add(within).ok_or_else(|| {
            CodecError::Malformed("CFB absolute mini-sector start overflows".into())
        })?;
        let end = start.checked_add(MINI_SECTOR_SIZE).ok_or_else(|| {
            CodecError::Malformed("CFB absolute mini-sector end overflows".into())
        })?.min(sector.end());
        sector.child(start, end).ok_or_else(|| {
            CodecError::Malformed("CFB mini sector crosses a regular-sector boundary".into())
        })
    }
}

impl CompoundState {
    fn parse(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        construction_storage: &mut ScopedReservation<'_>,
        root_chain_storage: &mut ScopedReservation<'_>,
    ) -> Result<Self, CodecError> {
        if bytes.get(..8) != Some(&MAGIC) {
            return malformed("input is not a CFB file");
        }
        let field = |offset, what| {
            View::u32_le_at(bytes, offset)
                .ok_or_else(|| CodecError::malformed(format_args!("truncated CFB {what}")))
        };
        if bytes.get(8..24) != Some(&[0; 16])
            || View::u16_le_at(bytes, 28) != Some(0xfffe)
        {
            return malformed("invalid CFB header identity or byte order");
        }
        let major_version = View::u16_le_at(bytes, 26)
            .ok_or_else(|| CodecError::Malformed("truncated CFB version".into()))?;
        let sector_shift = View::u16_le_at(bytes, 30)
            .ok_or_else(|| CodecError::Malformed("truncated CFB sector shift".into()))?;
        let version =
            CompoundVersion::from_header(major_version, sector_shift).ok_or_else(|| {
                CodecError::Malformed("unsupported or invalid CFB sector layout".into())
            })?;
        if View::u16_le_at(bytes, 32) != Some(6) || bytes.get(34..40) != Some(&[0; 6]) {
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
        // V4 fixes this header sector at 4096 bytes. The width check above
        // proves this fixed slice exists.
        if version == CompoundVersion::V4 && bytes[512..4096].iter().any(|byte| *byte != 0) {
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
                allocation_id_scratch.with_storage(|| {
                    ctx.reserve_capacity(&mut fat_sectors, 1, "CFB FAT sector slot")
                })?;
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
            if !construction_storage.with_storage(|| {
                ctx.insert_btree_set(&mut seen_difat, next_difat, "collect CFB DIFAT sector ids")
            })? {
                return malformed("CFB DIFAT chain is cyclic or out of range");
            }
            let data = sector(next_difat)
                .ok_or_else(|| CodecError::Malformed("CFB DIFAT sector is absent".into()))?;
            let mut free_seen = false;
            for index in 0..difat_entries {
                ctx.charge_work(1, "visit CFB DIFAT entries")?;
                let id = View::u32_le_at(data, index * 4)
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
                    allocation_id_scratch.with_storage(|| {
                        ctx.reserve_capacity(&mut fat_sectors, 1, "CFB FAT sector slot")
                    })?;
                    fat_sectors.push(id);
                }
            }
            next_difat = View::u32_le_at(data, difat_entries * 4)
                .ok_or_else(|| CodecError::Malformed("truncated CFB DIFAT link".into()))?;
        }
        if (difat_count == 0 && difat_start != END_OF_CHAIN)
            || (difat_count != 0 && next_difat != END_OF_CHAIN)
            || fat_sectors.len() != fat_count
            || ctx.any_by(
                &fat_sectors,
                |id| Ok(cadmpeg_core::decode::index_from_u32(*id) >= sector_count),
                "check CFB FAT sector bounds",
            )?
        {
            return malformed("CFB DIFAT does not match its declared FAT count");
        }
        let fat_sector_set = construction_storage.with_storage(|| ctx.collect_btree_set(
            fat_sectors.iter().copied(),
            "collect CFB FAT sector set",
        ))?;
        if fat_sector_set.len() != fat_sectors.len() {
            return malformed("duplicate CFB FAT sector");
        }
        if !ctx.is_disjoint_btree_set(
            &fat_sector_set,
            &seen_difat,
            "check CFB table role overlap",
        )? {
            return malformed("CFB sector has both FAT and DIFAT roles");
        }
        let fat_word_count = fat_count
            .checked_mul(sector_size / 4)
            .ok_or_else(|| CodecError::Malformed("CFB FAT word count overflow".into()))?;
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(fat_word_count),
            "parse CFB FAT words",
        )?;
        let mut fat = construction_storage.with_storage(|| {
            ctx.vector_storage(fat_word_count, "retain CFB FAT")
        })?;
        ctx.fold(&fat_sectors, (), |(), &id| {
            let data = sector(id)
                .ok_or_else(|| CodecError::Malformed("CFB FAT sector is absent".into()))?;
            if data.len() != sector_size {
                return malformed("CFB FAT sector is truncated");
            }
            // `sector_size` is 512 or 4096, both exact multiples of four;
            // the length check above proves this sector has that width.
            for raw in ctx.admit_iter(data.as_chunks::<4>().0, "decode CFB FAT words")? {
                construction_storage.with_storage(|| {
                    ctx.reserve_capacity(&mut fat, 1, "CFB FAT word slots")
                })?;
                fat.push(View::u32_le_at(raw, 0).ok_or_else(|| CodecError::Malformed("CFB FAT word is truncated".into()))?);
            }
            Ok(())
        }, "visit CFB FAT sectors")?;
        if fat.len() < sector_count {
            return malformed("CFB FAT does not address every physical sector");
        }
        if ctx.any_by(
            fat.get(sector_count..).unwrap_or_default(),
            |entry| Ok(*entry != FREE_SECTOR),
            "check CFB trailing FAT entries",
        )? {
            return malformed("CFB FAT entries past end-of-file are not free");
        }
        if ctx.any_by(
            &fat_sectors,
            |id| Ok(fat.get(cadmpeg_core::decode::index_from_u32(*id)) != Some(&FAT_SECTOR)),
            "check CFB FAT role markers",
        )? || ctx.any_by(
            &seen_difat,
            |id| Ok(fat.get(cadmpeg_core::decode::index_from_u32(*id)) != Some(&DIFAT_SECTOR)),
            "check CFB DIFAT role markers",
        )? {
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
        let directory_chain = construction_storage.with_storage(|| chain(
            ctx, &fat, sector_count, directory_start, directory_expected, ChainRole::Directory,
        ))?;
        let directory_records = StructuralRecords::new(
            ctx, bytes, sector_size, sector_count,
            directory_chain.as_ref().map(|chain| (&chain.first, chain.rest.as_slice())),
        )?;
        let directory = construction_storage.with_storage(|| {
            parse_directory_records(ctx, &directory_records, version)
        })?;
        validate_root(ctx, &directory)?;
        let mini_fat_chain = construction_storage.with_storage(|| chain(
            ctx, &fat, sector_count, mini_fat_start,
            NonZeroUsize::new(mini_fat_count).map(ChainLength::Declared), ChainRole::MiniFat,
        ))?;
        let mini_fat_records = StructuralRecords::<4>::new(
            ctx, bytes, sector_size, sector_count,
            mini_fat_chain.as_ref().map(|chain| (&chain.first, chain.rest.as_slice())),
        )?;
        let mini_fat_word_count = mini_fat_records.len();
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(mini_fat_word_count),
            "parse CFB mini FAT words",
        )?;
        let mut mini_fat = construction_storage.with_storage(|| {
            ctx.vector_storage(mini_fat_word_count, "retain CFB mini FAT")
        })?;
        for index in 0..mini_fat_word_count {
            ctx.charge_work(1, "decode CFB mini FAT words")?;
            let raw = mini_fat_records.get(index)?;
            construction_storage.with_storage(|| {
                ctx.reserve_capacity(&mut mini_fat, 1, "CFB mini FAT word slots")
            })?;
            mini_fat.push(View::u32_le_at(raw, 0).ok_or_else(|| CodecError::Malformed("CFB mini FAT word is truncated".into()))?);
        }
        let root = directory_root(&directory)?;
        let root_sectors = usize::try_from(root.size)
            .map_err(|_| {
                CodecError::Malformed("CFB root mini-stream size does not fit memory".into())
            })?
            .div_ceil(sector_size);
        let root_mini_chain = root_chain_storage.with_storage(|| chain(
            ctx, &fat, sector_count, root.start_sector,
            NonZeroUsize::new(root_sectors).map(ChainLength::Declared), ChainRole::RootMiniStream,
        ))?;
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
        metadata_storage: &mut ScopedReservation<'_>,
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
            (&mut output, metadata_storage),
        )?;

        ctx.fold(
            self.directory.get(1..).unwrap_or_default(),
            1_usize,
            |id, entry| {
                if matches!(entry, DirectorySlot::Live(_)) {
                    let reachable = match u32::try_from(id) {
                        Ok(id) => {
                            ctx.contains_btree_set(&reached.0, &id, "check CFB reachable directory id")?
                        }
                        Err(_) => false,
                    };
                    if !reachable {
                        return malformed("CFB directory contains an unreachable live entry");
                    }
                }
                Ok(id + 1)
            },
            "check CFB directory reachability",
        )?;
        Ok(output)
    }

    fn walk_tree(
        &self,
        ctx: &DecodeContext<'_>,
        snapshot_id: u64,
        root: u32,
        parent_index: Option<usize>,
        reached: &mut (BTreeSet<u32>, ScopedReservation<'_>),
        output: (&mut Vec<CompoundEntry>, &mut ScopedReservation<'_>),
    ) -> Result<(), CodecError> {
        let (output, metadata_storage) = output;
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
            let path = match parent_index {
                None => ctx.format_scoped_text(
                    metadata_storage,
                    format_args!("{}", entry.name.as_str()),
                    "retain CFB entry path",
                )?,
                Some(parent_index) => {
                    let Some(parent_path) = output
                        .get(parent_index)
                        .map(CompoundEntry::path)
                    else {
                        return malformed("CFB storage parent path index is out of range");
                    };
                    ctx.format_scoped_text(
                        metadata_storage,
                        format_args!("{parent_path}/{}", entry.name.as_str()),
                        "retain CFB entry path",
                    )?
                }
            };
            match entry.kind {
                DirectoryKind::Storage => {
                    let parent_index = output.len();
                    ctx.push_scoped_vec(
                        metadata_storage,
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
                        Some(parent_index),
                        reached,
                        (output, metadata_storage),
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
                    let empty = || StreamData::Empty(entry.start_sector);
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
                            let sectors = metadata_storage.with_storage(|| chain(
                                ctx, fat, count, entry.start_sector,
                                NonZeroUsize::new(expected).map(ChainLength::Declared), role,
                            ))?;
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
                    ctx.push_scoped_vec(
                        metadata_storage,
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
        for &sector in ctx
            .admit_iter(&self.fat_sectors, "visit CFB owned FAT sectors")?
            .chain(ctx.admit_iter(&self.difat_sectors, "visit CFB owned DIFAT sectors")?)
        {
            claim_structural(sector)?;
        }
        for chain in [
            &self.directory_chain,
            &self.mini_fat_chain,
            &self.root_mini_chain,
        ]
        .into_iter()
        .flatten()
        {
            chain.visit(ctx, &mut claim_structural)?;
        }
        let mut mini_used = BTreeSet::new();
        let root_size = directory_root(&self.directory)?.size;
        let mini_capacity = usize::try_from(root_size)
            .map_err(|_| {
                CodecError::Malformed("CFB root mini-stream size does not fit memory".into())
            })?
            .div_ceil(MINI_SECTOR_SIZE);

        ctx.fold(entries, (), |(), entry| {
            if let CompoundEntry::Stream(stream) = entry {
                let StreamData::Allocated {
                    allocation, chain, ..
                } = &stream.data
                else {
                    return Ok(());
                };
                let allocation = *allocation;
                let target = if allocation == CompoundAllocation::Regular {
                    &mut used
                } else {
                    &mut mini_used
                };
                let mut remaining = stream.logical_size();
                chain.visit(ctx, |sector| {
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
                    Ok(())
                })?;
            }
            Ok(())
        }, "visit CFB stream ownership")?;

        ctx.fold(&self.mini_fat, 0_usize, |ordinal, marker| {
            let sector = ordinal;
            let sector = u32::try_from(sector)
                .map_err(|_| CodecError::Malformed("CFB mini-sector id exceeds u32".into()))?;
            if !ctx.contains_btree_set(&mini_used, &sector, "check CFB mini FAT ownership")?
                && *marker != FREE_SECTOR
            {
                return malformed("unowned CFB mini sector is not marked free");
            }
            Ok(ordinal + 1)
        }, "visit CFB mini FAT ownership")?;

        ctx.fold(&self.fat[..self.sector_count.min(self.fat.len())], 0_usize, |ordinal, marker| {
            let sector = ordinal;
            let sector = u32::try_from(sector)
                .map_err(|_| CodecError::Malformed("CFB sector id exceeds u32".into()))?;
            if !ctx.contains_btree_set(&used, &sector, "check CFB FAT ownership")?
                && *marker != FREE_SECTOR
            {
                return malformed("unowned CFB sector is not marked free");
            }
            Ok(ordinal + 1)
        }, "visit CFB FAT ownership")?;
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

enum PrefixDirectoryAvailability<'ctx> {
    NotCompound,
    Incomplete,
    Malformed(&'static str),
    Ready {
        version: CompoundVersion,
        available: usize,
        directory_chain: Vec<u32>,
        storage: ScopedReservation<'ctx>,
    },
}

enum PrefixEvidenceFailure {
    Malformed(&'static str),
    Display(CodecError),
    Error(CodecError),
}

impl From<CodecError> for PrefixEvidenceFailure {
    fn from(error: CodecError) -> Self {
        Self::Error(error)
    }
}

impl From<ResourceLimit> for PrefixEvidenceFailure {
    fn from(error: ResourceLimit) -> Self {
        Self::Error(error.into())
    }
}

fn probe_directory_availability<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    prefix: View<'_>,
) -> Result<PrefixDirectoryAvailability<'ctx>, CodecError> {
    enum FatLoadStop {
        AvailablePrefix,
        Incomplete,
    }
    let prefix = prefix.window();
    if prefix.get(..8) != Some(&MAGIC) {
        return Ok(PrefixDirectoryAvailability::NotCompound);
    }
    let Some(major) = View::u16_le_at(prefix, 26) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(shift) = View::u16_le_at(prefix, 30) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(version) = CompoundVersion::from_header(major, shift) else {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "invalid CFB sector layout",
        ));
    };
    let sector_size = version.sector_size();
    if prefix.len() < sector_size {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    }
    if prefix.get(8..24) != Some(&[0; 16])
        || View::u16_le_at(prefix, 28) != Some(0xfffe)
        || View::u16_le_at(prefix, 32) != Some(6)
        || prefix.get(34..40) != Some(&[0; 6])
        || View::u32_le_at(prefix, 56) != Some(4096)
    {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "invalid CFB header",
        ));
    }
    if version == CompoundVersion::V4 && prefix[512..4096].iter().any(|byte| *byte != 0) {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "CFB v4 header padding is not zero",
        ));
    }
    let Some(fat_count) = View::u32_le_at(prefix, 44).and_then(|v| usize::try_from(v).ok()) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(directory_start) = View::u32_le_at(prefix, 48) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(directory_sector_count) = View::u32_le_at(prefix, 40) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(difat_start) = View::u32_le_at(prefix, 68) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    let Some(difat_count) = View::u32_le_at(prefix, 72).and_then(|v| usize::try_from(v).ok()) else {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    };
    if fat_count == 0
        || (version == CompoundVersion::V3 && directory_sector_count != 0)
        || (version == CompoundVersion::V4 && directory_sector_count == 0)
    {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "invalid CFB header counts",
        ));
    }

    let available = (prefix.len() - sector_size) / sector_size;
    let mut fat_sectors = ctx.scoped_vector_storage(0, "probe CFB FAT sectors")?;
    let mut header_free_seen = false;
    for index in 0..109 {
        let Some(id) = View::u32_le_at(prefix, 76 + index * 4) else {
            return Ok(PrefixDirectoryAvailability::Incomplete);
        };
        if id == FREE_SECTOR {
            header_free_seen = true;
        } else {
            if header_free_seen {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "non-free CFB header DIFAT entry follows a free entry",
                ));
            }
            if fat_sectors.0.len() == fat_count {
                return Ok(PrefixDirectoryAvailability::Malformed(
                    "CFB FAT ids exceed the declared count",
                ));
            }
            ctx.push_scoped_vec(
                &mut fat_sectors.1,
                &mut fat_sectors.0,
                id,
                "probe CFB FAT sectors",
            )?;
        }
    }
    let mut next_difat = difat_start;
    let difat_entries = sector_size / 4 - 1;
    let mut seen_difat_storage = ctx.reserve_scoped(0, "CFB probe visits")?;
    let mut seen_difat = BTreeSet::new();
    for _ in 0..difat_count {
        ctx.charge_work(1, "visit CFB probe DIFAT sector")?;
        if cadmpeg_core::decode::index_from_u32(next_difat) >= available {
            return Ok(PrefixDirectoryAvailability::Incomplete);
        }
        if !ctx.insert_scoped_btree_set(
            &mut seen_difat_storage,
            &mut seen_difat,
            next_difat,
            "CFB probe DIFAT visits",
            "CFB probe DIFAT visits",
        )? {
            return Ok(PrefixDirectoryAvailability::Malformed(
                "CFB DIFAT chain is cyclic",
            ));
        }
        let Some(raw) = sector_slice(prefix, sector_size, available, next_difat) else {
            return Ok(PrefixDirectoryAvailability::Incomplete);
        };
        let mut free_seen = false;
        for index in 0..difat_entries {
            ctx.charge_work(1, "visit CFB DIFAT entries")?;
            let Some(id) = View::u32_le_at(raw, index * 4) else {
                return Ok(PrefixDirectoryAvailability::Incomplete);
            };
            if id == FREE_SECTOR {
                free_seen = true;
            } else {
                if free_seen {
                    return Ok(PrefixDirectoryAvailability::Malformed(
                        "non-free CFB DIFAT entry follows a free entry",
                    ));
                }
                if fat_sectors.0.len() == fat_count {
                    return Ok(PrefixDirectoryAvailability::Malformed(
                        "CFB FAT ids exceed the declared count",
                    ));
                }
                ctx.push_scoped_vec(
                    &mut fat_sectors.1,
                    &mut fat_sectors.0,
                    id,
                    "probe CFB FAT sectors",
                )?;
            }
        }
        let Some(next) = View::u32_le_at(raw, difat_entries * 4) else {
            return Ok(PrefixDirectoryAvailability::Incomplete);
        };
        next_difat = next;
    }
    if (difat_count == 0 && difat_start != END_OF_CHAIN)
        || (difat_count != 0 && next_difat != END_OF_CHAIN)
    {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "CFB DIFAT chain length does not match the header",
        ));
    }
    if fat_sectors.0.len() != fat_count {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "CFB DIFAT does not match its declared FAT count",
        ));
    }

    let mut fat = (Vec::new(), ctx.reserve_scoped(0, "CFB probe FAT words")?);
    let mut loaded_fat_count = 0;
    let fat_stop = ctx.find_map(&fat_sectors.0, |&id| {
        if cadmpeg_core::decode::index_from_u32(id) >= available {
            return Ok(Some(FatLoadStop::AvailablePrefix));
        }
        let Some(raw) = sector_slice(prefix, sector_size, available, id) else {
            return Ok(Some(FatLoadStop::Incomplete));
        };
        // Available sectors contain complete words, so the admitted width is exact.
        ctx.reserve_scoped_vec(&mut fat.1, &mut fat.0, raw.len() / 4, "CFB probe FAT words")?;
        for word in ctx.admit_iter(raw.as_chunks::<4>().0, "decode CFB probe FAT words")? {
            fat.1.with_storage(|| {
                ctx.reserve_capacity(&mut fat.0, 1, "CFB probe FAT word slot")
            })?;
            fat.0.push(View::u32_le_at(word, 0).ok_or_else(|| CodecError::Malformed("CFB probe FAT word is truncated".into()))?);
        }
        loaded_fat_count += 1;
        Ok((loaded_fat_count == fat_count).then_some(FatLoadStop::AvailablePrefix))
    }, "visit CFB FAT sectors")?;
    if matches!(fat_stop, Some(FatLoadStop::Incomplete)) {
        return Ok(PrefixDirectoryAvailability::Incomplete);
    }
    let fat_role_stop = ctx.find_map(
        &fat_sectors.0[..loaded_fat_count.min(fat_sectors.0.len())],
        |id| Ok(match fat.0.get(cadmpeg_core::decode::index_from_u32(*id)) {
            None => Some(PrefixDirectoryAvailability::Incomplete),
            Some(&FAT_SECTOR) => None,
            Some(_) => Some(PrefixDirectoryAvailability::Malformed(
                "CFB allocation sector has the wrong role marker",
            )),
        }),
        "check CFB probe FAT roles",
    )?;
    if let Some(stop) = fat_role_stop {
        return Ok(stop);
    }
    let difat_role_stop = ctx.find_map(
        &seen_difat,
        |id| Ok(match fat.0.get(cadmpeg_core::decode::index_from_u32(*id)) {
            None => Some(PrefixDirectoryAvailability::Incomplete),
            Some(&DIFAT_SECTOR) => None,
            Some(_) => Some(PrefixDirectoryAvailability::Malformed(
                "CFB allocation sector has the wrong role marker",
            )),
        }),
        "check CFB probe DIFAT roles",
    )?;
    if let Some(stop) = difat_role_stop {
        return Ok(stop);
    }
    drop((seen_difat, seen_difat_storage));
    drop(fat_sectors);
    let expected_directory_count = if version == CompoundVersion::V4 {
        Some(cadmpeg_core::decode::index_from_u32(directory_sector_count))
    } else if directory_sector_count == 0 {
        None
    } else {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "CFB v3 declares directory sector count",
        ));
    };

    let mut directory_chain_storage = ctx.reserve_scoped(0, "CFB probe directory chain")?;
    let mut directory_chain = Vec::new();
    let mut seen_directory_storage = ctx.reserve_scoped(0, "CFB probe visits")?;
    let mut seen_directory = BTreeSet::new();
    let mut current = directory_start;
    loop {
        ctx.charge_work(1, "visit CFB probe directory chain")?;
        if cadmpeg_core::decode::index_from_u32(current) >= available {
            return Ok(PrefixDirectoryAvailability::Incomplete);
        }
        let Some(&next) = fat.0.get(cadmpeg_core::decode::index_from_u32(current)) else {
            return Ok(if loaded_fat_count < fat_count {
                PrefixDirectoryAvailability::Incomplete
            } else {
                PrefixDirectoryAvailability::Malformed(
                    "CFB FAT does not address the directory sector",
                )
            });
        };
        if !ctx.insert_scoped_btree_set(
            &mut seen_directory_storage,
            &mut seen_directory,
            current,
            "CFB probe directory visits",
            "CFB probe directory visits",
        )? {
            return Ok(PrefixDirectoryAvailability::Malformed(
                "CFB directory chain is cyclic",
            ));
        }
        ctx.push_scoped_vec(
            &mut directory_chain_storage,
            &mut directory_chain,
            current,
            "CFB probe directory chain",
        )?;
        if next == END_OF_CHAIN {
            break;
        }
        if next >= DIFAT_SECTOR {
            return Ok(PrefixDirectoryAvailability::Malformed(
                "CFB directory chain has an invalid terminator",
            ));
        }
        current = next;
    }
    if expected_directory_count.is_some_and(|count| count != directory_chain.len()) {
        return Ok(PrefixDirectoryAvailability::Malformed(
            "CFB directory chain length does not match the header",
        ));
    }
    drop((fat, seen_directory, seen_directory_storage));
    Ok(PrefixDirectoryAvailability::Ready {
        version,
        available,
        directory_chain,
        storage: directory_chain_storage,
    })
}

fn malformed_prefix_probe<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    detail: &'static str,
) -> Result<(CompoundPrefixProbe, ScopedReservation<'ctx>), CodecError> {
    let storage = ctx.reserve_scoped(0, "CFB prefix probe result")?;
    Ok((CompoundPrefixProbe::Malformed(detail.into()), storage))
}

fn malformed_prefix_probe_error<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    error: CodecError,
) -> Result<(CompoundPrefixProbe, ScopedReservation<'ctx>), CodecError> {
    if matches!(&error, CodecError::ResourceLimit(_)) {
        return Err(error);
    }
    let storage = ctx.reserve_scoped(0, "CFB prefix probe result")?;
    Ok((CompoundPrefixProbe::Malformed(error.to_string()), storage))
}

impl CompoundPrefixProbe {
    /// Parses complete prefix structures with caller-admitted scratch storage.
    pub fn inspect_with_context<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        let (version, available, directory_chain) =
            match probe_directory_availability(ctx, prefix)? {
                PrefixDirectoryAvailability::NotCompound => {
                    return Ok((
                        Self::NotCompound,
                        ctx.reserve_scoped(0, "CFB prefix probe result")?,
                    ));
                }
                PrefixDirectoryAvailability::Incomplete => {
                    return Ok((
                        Self::Incomplete,
                        ctx.reserve_scoped(0, "CFB prefix probe result")?,
                    ));
                }
                PrefixDirectoryAvailability::Malformed(detail) => {
                    return malformed_prefix_probe(ctx, detail);
                }
                PrefixDirectoryAvailability::Ready {
                    version,
                    available,
                    directory_chain,
                    storage,
                } => (version, available, (directory_chain, storage)),
            };
        let prefix = prefix.window();
        let directory_records = StructuralRecords::new(
            ctx, prefix, version.sector_size(), available, directory_chain.0.split_first(),
        )?;
        let parsed = ctx.with_scoped_storage("parse CFB prefix directory", || {
            parse_directory_records(ctx, &directory_records, version)
        });
        drop(directory_chain);
        let directory = match parsed {
            Ok(parsed) => parsed,
            Err(error) if matches!(&error, CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => return malformed_prefix_probe_error(ctx, error),
        };
        let root = match directory_root(&directory.0) {
            Ok(root) => root,
            Err(error) => {
                drop(directory);
                return malformed_prefix_probe_error(ctx, error);
            }
        };
        let root_child = root.child;
        if let Err(error) = validate_root(ctx, &directory.0) {
            drop(directory);
            if matches!(&error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            return malformed_prefix_probe_error(ctx, error);
        }
        if let Err(error) = validate_sibling_tree(ctx, &directory.0, root_child) {
            drop(directory);
            if matches!(&error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            return malformed_prefix_probe_error(ctx, error);
        }
        let path_phase = ctx.with_scoped_storage("CFB prefix evidence scratch", || {
            let mut names = ctx.scoped_vector_storage(0, "CFB probe paths")?;
            let traversal = (|| -> Result<(), PrefixEvidenceFailure> {
                let mut pending = ctx.collection_vec(
                    usize::from(root_child != NO_STREAM),
                    "CFB probe pending links",
                )?;
                if root_child != NO_STREAM {
                    ctx.reserve_capacity(&mut pending, 1, "CFB probe pending slot")?;
                    pending.push((root_child, None));
                }
                let mut seen_storage = ctx.reserve_scoped(0, "CFB probe visits")?;
                let mut seen = BTreeSet::new();
                while let Some((id, parent_index)) = pending.pop() {
                    ctx.charge_work(1, "visit CFB probe path")?;
                    let Some(entry) = directory
                        .0
                        .get(cadmpeg_core::decode::index_from_u32(id))
                        .and_then(DirectorySlot::live)
                    else {
                        return Err(PrefixEvidenceFailure::Malformed(
                            "CFB directory link is out of range",
                        ));
                    };
                    if !ctx.insert_scoped_btree_set(
                        &mut seen_storage,
                        &mut seen,
                        id,
                        "CFB probe live entries",
                        "CFB probe live entries",
                    )? {
                        return Err(PrefixEvidenceFailure::Malformed(
                            "CFB directory link cycle",
                        ));
                    }
                    let path = match parent_index {
                        None => ctx.copy_scoped_text(
                            entry.name.as_str(),
                            &mut names.1,
                            "CFB probe path",
                        )?,
                        Some(parent_index) => {
                            let Some(parent) = names.0.get(parent_index) else {
                                return Err(PrefixEvidenceFailure::Error(
                                    CodecError::Malformed(
                                        "CFB probe parent path index is out of range".into(),
                                    ),
                                ));
                            };
                            ctx.format_scoped_text(
                                &mut names.1,
                                format_args!("{parent}/{}", entry.name.as_str()),
                                "CFB probe path",
                            )?
                        }
                    };
                    let path_index = names.0.len();
                    ctx.push_scoped_vec(
                        &mut names.1,
                        &mut names.0,
                        path,
                        "CFB probe paths",
                    )?;
                    if entry.left != NO_STREAM {
                        ctx.push_vec(
                            &mut pending,
                            (entry.left, parent_index),
                            "CFB probe pending links",
                        )?;
                    }
                    if entry.right != NO_STREAM {
                        ctx.push_vec(
                            &mut pending,
                            (entry.right, parent_index),
                            "CFB probe pending links",
                        )?;
                    }
                    if entry.kind == DirectoryKind::Storage {
                        if let Err(error) = validate_sibling_tree(ctx, &directory.0, entry.child) {
                            if matches!(&error, CodecError::ResourceLimit(_)) {
                                return Err(PrefixEvidenceFailure::Error(error));
                            }
                            return Err(PrefixEvidenceFailure::Display(error));
                        }
                        if entry.child != NO_STREAM {
                            ctx.push_vec(
                                &mut pending,
                                (entry.child, Some(path_index)),
                                "CFB probe pending links",
                            )?;
                        }
                    }
                }

                let reachability = ctx.fold(
                    directory.0.get(1..).unwrap_or_default(),
                    1_usize,
                    |id, entry| {
                        if matches!(entry, DirectorySlot::Live(_)) {
                            let reachable = match u32::try_from(id) {
                                Ok(id) => ctx.contains_btree_set(
                                    &seen,
                                    &id,
                                    "check CFB probe reachable id",
                                )?,
                                Err(_) => false,
                            };
                            if !reachable {
                                return malformed("CFB directory contains an unreachable live entry");
                            }
                        }
                        Ok(id + 1)
                    },
                    "visit CFB probe reachability",
                );
                match reachability {
                    Ok(_) => {},
                    // The callback's only structural failure is this fixed
                    // reachability diagnostic; admission failures remain typed.
                    Err(CodecError::Malformed(_)) => return Err(PrefixEvidenceFailure::Malformed(
                        "CFB directory contains an unreachable live entry",
                    )),
                    Err(error) => return Err(PrefixEvidenceFailure::Error(error)),
                }
                Ok(())
            })();
            Ok::<_, CodecError>((names, traversal))
        })?;
        let (names, traversal) = path_phase.0;
        drop(path_phase.1);
        match traversal {
            Ok(()) => {
                drop(directory);
                Ok((Self::DirectoryEvidence(names.0), names.1))
            }
            Err(PrefixEvidenceFailure::Malformed(detail)) => {
                drop((names, directory));
                malformed_prefix_probe(ctx, detail)
            }
            Err(PrefixEvidenceFailure::Display(error)) => {
                drop((names, directory));
                malformed_prefix_probe_error(ctx, error)
            }
            Err(PrefixEvidenceFailure::Error(error)) => {
                drop((names, directory));
                Err(error)
            }
        }
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
        let availability = probe_directory_availability(ctx, View::over_retained(&bytes))?;
        let incomplete = matches!(
            &availability,
            PrefixDirectoryAvailability::Incomplete
        );
        drop(availability);
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

#[cfg(test)]
fn parse_directory(
    ctx: &DecodeContext<'_>, bytes: &[u8], version: CompoundVersion,
) -> Result<Vec<DirectorySlot>, CodecError> {
    let (records, remainder) = bytes.as_chunks::<{ directory_layout::LEN }>();
    if !remainder.is_empty() {
        return malformed("CFB directory stream has a partial entry");
    }
    parse_directory_records(ctx, &StructuralRecords::Contiguous(records), version)
}

fn parse_directory_records(
    ctx: &DecodeContext<'_>,
    records: &StructuralRecords<'_, { directory_layout::LEN }>,
    version: CompoundVersion,
) -> Result<Vec<DirectorySlot>, CodecError> {
    let entry_count = records.len();
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(entry_count),
        "parse CFB directory entries",
    )?;
    let mut entries = ctx.vector_storage(entry_count, "parse CFB directory entries")?;

    for index in 0..entry_count {
        ctx.charge_work(1, "visit CFB directory records")?;
        let raw = records.get(index)?;
        let object_type = raw[directory_layout::OBJECT_TYPE];
        if object_type == 0 {
            if raw[..directory_layout::LEFT] != [0; directory_layout::LEFT]
                || raw[directory_layout::LEFT..directory_layout::CLSID]
                    != [0xff; directory_layout::CLSID - directory_layout::LEFT]
                || raw[directory_layout::CLSID..] != [0; directory_layout::LEN - directory_layout::CLSID]
            {
                return malformed("invalid CFB unallocated directory entry");
            }
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
        let child = View::u32_le_at(raw, directory_layout::CHILD).ok_or_else(|| CodecError::Malformed("CFB directory field is truncated".into()))?;
        let start_sector =
            View::u32_le_at(raw, directory_layout::START_SECTOR).ok_or_else(|| CodecError::Malformed("CFB directory field is truncated".into()))?;
        let mut size = View::u64_le_at(raw, directory_layout::STREAM_SIZE).ok_or_else(|| CodecError::Malformed("CFB directory field is truncated".into()))?;
        if version == CompoundVersion::V3 {
            size &= 0xffff_ffff;
        }
        match kind {
            DirectoryKind::Stream
                if child != NO_STREAM
                    || raw[directory_layout::CLSID..directory_layout::STATE_BITS] != [0; 16]
                    || raw[directory_layout::CREATION_TIME..directory_layout::START_SECTOR] != [0; 16] =>
            {
                return malformed("invalid CFB stream directory fields");
            }
            DirectoryKind::Storage if start_sector != 0 || size != 0 => {
                return malformed("invalid CFB storage directory fields");
            }
            DirectoryKind::Root
                if raw[directory_layout::CREATION_TIME..directory_layout::MODIFIED_TIME] != [0; 8] =>
            {
                return malformed("invalid CFB root directory fields");
            }
            _ => {}
        }
        if version == CompoundVersion::V3
            && matches!(kind, DirectoryKind::Stream | DirectoryKind::Root)
            && size > V3_MAX_STREAM_SIZE
        {
            return malformed("CFB v3 stream size exceeds 0x80000000");
        }
        let name_len = usize::from(View::u16_le_at(raw, directory_layout::NAME_LENGTH)
            .ok_or_else(|| CodecError::Malformed("CFB directory name length is truncated".into()))?);
        let name = {
            if !(2..=directory_layout::NAME_LENGTH - directory_layout::NAME).contains(&name_len)
                || !name_len.is_multiple_of(2)
                || raw[directory_layout::NAME + name_len - 2..directory_layout::NAME + name_len] != [0, 0]
            {
                return malformed("invalid CFB directory name length or terminator");
            }
            // The field is fixed at 64 bytes. Only counted content precedes
            // the required final terminator; uncounted padding is ignored.
            let name_field = raw[directory_layout::NAME..directory_layout::NAME_LENGTH]
                .as_chunks::<{ directory_layout::NAME_LENGTH - directory_layout::NAME }>().0[0];
            for index in 0..(directory_layout::NAME_LENGTH - directory_layout::NAME) / 2 {
                if index < (name_len - 2) / 2 && name_field[index * 2..index * 2 + 2] == [0, 0] {
                    return malformed("CFB directory name has an earlier terminator");
                }
            }
            let name =
                ctx.utf16le_text(&name_field, (name_len - 2) / 2, false, "decode CFB directory name")?;
            DirectoryName::new(ctx, name)?
        };
        let color = match raw[directory_layout::COLOR] {
            0 => DirectoryColor::Red,
            1 => DirectoryColor::Black,
            _ => return malformed("invalid CFB directory node color"),
        };
        let entry = DirectorySlot::Live(LiveEntry {
            name,
            kind,
            color,
            left: View::u32_le_at(raw, directory_layout::LEFT).ok_or_else(|| CodecError::Malformed("CFB directory field is truncated".into()))?,
            right: View::u32_le_at(raw, directory_layout::RIGHT).ok_or_else(|| CodecError::Malformed("CFB directory field is truncated".into()))?,
            child,
            start_sector,
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
    if ctx.any_by(
        directory.get(1..).unwrap_or_default(),
        |entry| {
            Ok(entry
                .live()
                .is_some_and(|entry| entry.kind == DirectoryKind::Root))
        },
        "scan CFB root entries",
    )? {
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
    if lower
        .map(|name| cfb_name_cmp(ctx, name, entry.name.as_str()))
        .transpose()?
        .is_some_and(|order| order != Ordering::Less)
        || upper
            .map(|name| cfb_name_cmp(ctx, entry.name.as_str(), name))
            .transpose()?
            .is_some_and(|order| order != Ordering::Less)
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
    let left_len = ctx
        .admit_iter(left, "compare CFB sibling names")?
        .encode_utf16()
        .count();
    let right_len = ctx
        .admit_iter(right, "compare CFB sibling names")?
        .encode_utf16()
        .count();
    let length_order = left_len.cmp(&right_len);
    if length_order != Ordering::Equal {
        return Ok(length_order);
    }
    // Each iterator step yields one UTF-16 unit from at most one scalar.
    // Stop admission at the first differing pair; equal names reach the end
    // probe. The complete length scans above remain separate actual work.
    Ok(ctx.find_map(
        left.encode_utf16().zip(right.encode_utf16()),
        |(left, right)| {
            let order = cfb_upper_unit(left).cmp(&cfb_upper_unit(right));
            Ok((order != Ordering::Equal).then_some(order))
        },
        "compare CFB sibling names",
    )?.unwrap_or(Ordering::Equal))
}

fn path_key(ctx: &DecodeContext<'_>, path: &str) -> Result<Vec<Vec<u16>>, CodecError> {
    let mut components = Vec::new();
    let mut component = Vec::new();
    let mut characters = path.chars();
    while let Some(character) = ctx.next_charged(&mut characters, "scan CFB path key")? {
        if character == '/' {
            ctx.push_vec(
                &mut components,
                std::mem::take(&mut component),
                "CFB path key components",
            )?;
        } else {
            let mut encoded = [0_u16; 2];
            let units = character.encode_utf16(&mut encoded);
            // A scalar always yields one unit or one surrogate pair.
            ctx.push_vec(&mut component, cfb_upper_unit(units[0]), "CFB path key units")?;
            if let Some(&second) = units.get(1) {
                ctx.push_vec(&mut component, cfb_upper_unit(second), "CFB path key units")?;
            }
        }
    }
    ctx.push_vec(&mut components, component, "CFB path key components")?;
    Ok(components)
}

fn cfb_upper_unit(unit: u16) -> u16 {
    // These Greek characters have one-unit uppercase values. Full uppercase
    // expands them, so map their simple uppercase values before conversion.
    let simple_exception = match unit {
        0x1f80..=0x1f87 | 0x1f90..=0x1f97 | 0x1fa0..=0x1fa7 => unit + 8,
        0x1fb3 => 0x1fbc,
        0x1fc3 => 0x1fcc,
        0x1ff3 => 0x1ffc,
        _ => unit,
    };
    if simple_exception != unit {
        return simple_exception;
    }
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
            return Err(CodecError::malformed(format_args!(
                "empty CFB {role} has an invalid start sector"
            )));
        }
        Some(ChainLength::Declared(count)) => Some(count),
        Some(ChainLength::Unbounded) => None,
    };
    if start == END_OF_CHAIN {
        return if expected.is_some() {
            Err(CodecError::malformed(format_args!(
                "CFB {role} chain length does not match its declaration"
            )))
        } else {
            Err(CodecError::malformed(format_args!("empty CFB {role}")))
        };
    }
    if expected.is_some_and(|count| count.get() > sector_count) {
        return Err(CodecError::malformed(format_args!(
            "CFB {role} chain length exceeds available sectors"
        )));
    }
    if cadmpeg_core::decode::index_from_u32(start) >= sector_count {
        return Err(CodecError::malformed(format_args!(
            "CFB {role} chain is cyclic, overlong, or out of range"
        )));
    }
    let limit = expected.map_or(sector_count, NonZeroUsize::get);
    if let Some(count) = expected {
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(count.get()),
            "retain CFB sector chain",
        )?;
    }
    let mut traversal_scratch = ctx.reserve_scoped(0, "walk CFB sector chain")?;
    let mut output = ctx.scoped_vector_storage::<u32>(
        expected.map_or(0, |count| count.get() - 1),
        "retain CFB sector chain",
    )?;
    let mut seen = BTreeSet::new();
    let mut current = start;
    while current != END_OF_CHAIN {
        ctx.charge_work(1, "scan CFB sector chain")?;
        if cadmpeg_core::decode::index_from_u32(current) >= sector_count || seen.len() == limit {
            return Err(CodecError::malformed(format_args!(
                "CFB {role} chain is cyclic, overlong, or out of range"
            )));
        }
        if !ctx.insert_scoped_btree_set(
            &mut traversal_scratch,
            &mut seen,
            current,
            "compare CFB visited sectors",
            "walk CFB sector chain",
        )? {
            return Err(CodecError::malformed(format_args!(
                "CFB {role} chain is cyclic, overlong, or out of range"
            )));
        }
        if expected.is_none() {
            if current == start {
                ctx.charge_collection_items(1, "retain CFB sector chain")?;
            } else {
                output.1.with_storage(|| {
                    ctx.push_vec(&mut output.0, current, "retain CFB sector chain")
                })?;
            }
        } else if current != start {
            output
                .1
                .with_storage(|| ctx.reserve_capacity(&mut output.0, 1, "CFB sector chain slot"))?;
            output.0.push(current);
        }
        current = *fat
            .get(cadmpeg_core::decode::index_from_u32(current))
            .ok_or_else(|| CodecError::malformed(format_args!("CFB {role} FAT link is absent")))?;
        if matches!(current, FREE_SECTOR | FAT_SECTOR | DIFAT_SECTOR) {
            return Err(CodecError::malformed(format_args!(
                "CFB {role} chain enters a reserved sector role"
            )));
        }
    }
    if expected.is_some_and(|count| 1 + output.0.len() != count.get()) {
        return Err(CodecError::malformed(format_args!(
            "CFB {role} chain length does not match its declaration"
        )));
    }
    drop(seen);
    drop(traversal_scratch);
    output.1.commit_value(Some(SectorChain {
        first: start,
        rest: output.0,
    }))
}

/// Fixed-width records borrowed in allocation-chain order. Structural extents
/// are validated before any record grammar or output allocation.
enum StructuralRecords<'a, const WIDTH: usize> {
    #[cfg(test)]
    Contiguous(&'a [[u8; WIDTH]]),
    Sectors {
        bytes: &'a [u8],
        sector_size: usize,
        sector_count: usize,
        sectors: Option<(&'a u32, &'a [u32])>,
        records: usize,
    },
}

impl<'a, const WIDTH: usize> StructuralRecords<'a, WIDTH> {
    fn new(
        ctx: &DecodeContext<'_>, bytes: &'a [u8], sector_size: usize,
        sector_count: usize, sectors: Option<(&'a u32, &'a [u32])>,
    ) -> Result<Self, CodecError> {
        // The two production widths, 4 and 128, tile both header-selected
        // sector widths, 512 and 4096. No record crosses a sector boundary.
        if WIDTH == 0 || !sector_size.is_multiple_of(WIDTH) {
            return malformed("CFB structural records do not tile their sectors");
        }
        let (first, rest) = sectors.map_or((None, &[][..]), |(first, rest)| (Some(first), rest));
        let records = rest.len().checked_add(usize::from(first.is_some()))
            .and_then(|count| count.checked_mul(sector_size / WIDTH))
            .ok_or_else(|| CodecError::Malformed("CFB structural record count overflow".into()))?;
        let validate = |sector| -> Result<(), CodecError> {
            let data = sector_slice(bytes, sector_size, sector_count, sector)
                .ok_or_else(|| CodecError::Malformed("CFB sector is absent".into()))?;
            if data.len() != sector_size {
                return malformed("CFB structural sector is truncated");
            }
            Ok(())
        };
        if let Some(&sector) = first { validate(sector)?; }
        ctx.fold(rest, (), |(), &sector| validate(sector), "walk CFB structural sectors")?;
        Ok(Self::Sectors { bytes, sector_size, sector_count, sectors, records })
    }

    fn len(&self) -> usize {
        match self {
            #[cfg(test)]
            Self::Contiguous(records) => records.len(),
            Self::Sectors { records, .. } => *records,
        }
    }

    fn get(&self, index: usize) -> Result<&'a [u8; WIDTH], CodecError> {
        let missing = || CodecError::Malformed("CFB structural record is absent".into());
        match self {
            #[cfg(test)]
            Self::Contiguous(records) => records.get(index).ok_or_else(missing),
            Self::Sectors { bytes, sector_size, sector_count, sectors, records } => {
                if index >= *records { return Err(missing()); }
                let records_per_sector = sector_size / WIDTH;
                let sector_index = index / records_per_sector;
                let (first, rest) = sectors.ok_or_else(missing)?;
                let sector = if sector_index == 0 { first }
                    else { rest.get(sector_index - 1).ok_or_else(missing)? };
                let bytes = sector_slice(bytes, *sector_size, *sector_count, *sector)
                    .ok_or_else(missing)?;
                bytes.as_chunks::<WIDTH>().0.get(index % records_per_sector).ok_or_else(missing)
            }
        }
    }
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

fn malformed<T>(message: impl Into<String>) -> Result<T, CodecError> {
    Err(CodecError::Malformed(message.into()))
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::io::{self, Read};

    use cadmpeg_core::container::ContainerRole;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_test_support::bytes::{put_u16, put_u32};
    use crate::layout::directory_entry as directory_layout;

    use super::{
        cfb_name_cmp, cfb_upper_unit, chain, parse_directory, path_key, range_lock_sector,
        read_detection_prefix, validate_sibling_tree, ChainLength, ChainRole, CompoundEntry,
        CompoundPrefixProbe, CompoundSnapshot, CompoundState, CompoundVersion, DirectorySlot,
        DIFAT_SECTOR, END_OF_CHAIN, FAT_SECTOR, FREE_SECTOR, MAGIC, NO_STREAM, RANGE_LOCK_END,
    };

    mod bounded_traversal;
    mod structural_records;
    mod chains;
    mod directory_fields;
    mod fat_coverage;
    mod fixed_work;
    mod name_grammar;
    mod prefix;
    mod snapshot_lifetime;
    mod stream_extents;
    mod stream_open;

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

    fn parse_state(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<CompoundState, CodecError> {
        let mut construction = ctx.reserve_scoped(0, "test CFB construction state")?;
        let mut root_chain = ctx.reserve_scoped(0, "test CFB root mini-chain")?;
        let state = CompoundState::parse(ctx, bytes, &mut construction, &mut root_chain)?;
        construction.absorb(&mut root_chain)?;
        construction.commit_value(state)
    }

    fn build_entries(
        ctx: &DecodeContext<'_>, state: &CompoundState, snapshot_id: u64,
    ) -> Result<Vec<CompoundEntry>, CodecError> {
        let mut metadata = ctx.reserve_scoped(0, "test CFB entry metadata")?;
        let entries = state.build_entries(ctx, snapshot_id, &mut metadata)?;
        metadata.commit_value(entries)
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
    fn sector_chain_fixed_error_uses_no_work_or_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let error = chain(&ctx, &[], 0, 0, None, ChainRole::Directory)
            .expect_err("fixed structural diagnostic");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "empty CFB directory has an invalid start sector"));
        assert!(ctx.resource_refusal().is_none());
    }

    #[test]
    fn directory_record_visits_refuse_before_decoding() {
        let mut bytes = [0u8; 128];
        initialize_empty_directory_entries(&mut bytes);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) =
            super::parse_directory(&ctx, &bytes, super::CompoundVersion::V3)
                .expect_err("record visits")
        else {
            panic!("refusal")
        };
        assert_eq!(first.operation, "visit CFB directory records");
        let CodecError::ResourceLimit(repeated) =
            ctx.charge_work(1, "later").expect_err("fused refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(first, repeated);
    }

    #[test]
    fn structural_records_use_borrowed_parts_and_refuse_before_tail_validation() {
        let bytes = [0xabu8; 1536];
        let first = 1;
        let rest = [0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let records = super::StructuralRecords::<128>::new(&ctx, &bytes, 512, 2, Some((&first, &rest)))
            .expect("borrowed structural records");
        let logical = (0..records.len()).flat_map(|index| records.get(index).expect("record").iter().copied()).collect::<Vec<_>>();
        assert_eq!(logical, [0xabu8; 1024]);
        assert_eq!(records.get(0).expect("first record").as_ptr(), bytes[1024..].as_ptr());
        assert_eq!(records.get(4).expect("next sector record").as_ptr(), bytes[512..].as_ptr());
        assert_eq!(ctx.resource_refusal(), None);
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert_eq!(super::StructuralRecords::<128>::new(&ctx, &bytes, 512, 2, None)
            .expect("empty chain").len(), 0);
        assert_eq!(super::StructuralRecords::<128>::new(&ctx, &bytes, 512, 2, Some((&first, &[])))
            .expect("fixed first uses no traversal or copy work").len(), 4);
        let error = match super::StructuralRecords::<128>::new(&ctx, &bytes, 512, 2, Some((&first, &rest))) {
            Ok(_) => panic!("variable tail needs admission"), Err(error) => error,
        };
        let CodecError::ResourceLimit(first) = error else { panic!("tail refusal") };
        assert_eq!(first.operation, "walk CFB structural sectors");
        assert_eq!((first.used, first.additional), (0, 1));
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(1, "later").expect_err("fused refusal")
            else { panic!("refusal") };
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
            parse_state(ctx, &file).expect("allocation tables parse")
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
            0,
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
            0,
            0,
        );
        state.directory = with_context(&directory, &DecodePolicy::service(), |ctx| {
            parse_directory(ctx, &directory, CompoundVersion::V3).expect("directory parses")
        });
        with_context(&[], &DecodePolicy::service(), |ctx| {
            assert_eq!(
                build_entries(ctx, &state, 1)
                    .expect("nested storages parse")
                    .len(),
                2
            );
        });
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 2;
        let error = with_context(&[], &policy, |ctx| build_entries(ctx, &state, 1))
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
    fn storage_parent_path_uses_output_ordinal_at_exact_scratch_peak() {
        let file = fixture();
        let mut state = with_context(&file, &DecodePolicy::service(), |ctx| {
            parse_state(ctx, &file).expect("allocation tables")
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
            0,
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
            build_entries(ctx, &state, 1).expect("entries")
        });
        assert_eq!(entries[1].path(), format!("{name}/B"));
        let node_bytes = 11 * (std::mem::size_of::<u32>() + std::mem::size_of::<()>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<u32>()
                .max(std::mem::align_of::<()>())
                .max(std::mem::align_of::<usize>());
        let pending_bytes = std::mem::size_of::<u32>();
        // The reached set holds one B-tree node for both ids. Its outer
        // worklist keeps one u32 live while the child tree validates another
        // single-node B-tree. The first path and four output slots remain
        // live during that child validation.
        let metadata_bytes = name.len() + 4 * std::mem::size_of::<CompoundEntry>();
        let exact_peak = metadata_bytes + 2 * node_bytes + pending_bytes;
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(exact_peak);
        let bounded_entries = with_context(&[], &policy, |ctx| build_entries(ctx, &state, 1))
            .expect("owned parent path fits the derived scoped peak");
        assert_eq!(bounded_entries[1].path(), format!("{name}/B"));

        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(exact_peak - 1);
        let error = with_context(&[], &policy, |ctx| build_entries(ctx, &state, 1))
            .expect_err("one byte below the child validation peak refuses");
        let expected_used = cadmpeg_core::decode::u64_from_index(metadata_bytes + node_bytes + pending_bytes);
        let expected_additional = cadmpeg_core::decode::u64_from_index(node_bytes);
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                    && limit.operation == "validate CFB sibling nodes"
                    && limit.used == expected_used
                    && limit.additional == expected_additional
        ));
    }

    #[test]
    fn sector_ownership_validation_uses_the_callers_resources() {
        use cadmpeg_core::decode::ResourceDimension;
        let file = fixture();
        let state = with_context(&file, &DecodePolicy::service(), |ctx| {
            parse_state(ctx, &file).expect("allocation tables parse")
        });
        let entries = with_context(&[], &DecodePolicy::service(), |ctx| {
            build_entries(ctx, &state, 1).expect("directory entries")
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
    fn read_detection_prefix_stops_before_directory_materialization() {
        let file = fixture_v4();
        let mut policy = DecodePolicy::service();
        let sector_size = 4096;
        let id_node_bytes = 11 * std::mem::size_of::<u32>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
        // FAT loading overlaps one FAT id with the FAT words. Directory
        // traversal then overlaps those words with one visit node and the
        // four-slot minimum capacity of its chain vector.
        let fat_loading_peak = std::mem::size_of::<u32>()
            + (sector_size / 4) * std::mem::size_of::<u32>();
        let directory_chain_peak = (sector_size / 4) * std::mem::size_of::<u32>()
            + id_node_bytes
            + 4 * std::mem::size_of::<u32>();
        let stage_peak = fat_loading_peak.max(directory_chain_peak);
        // Input acquisition reserves exact byte capacity. Its reallocation
        // overlap cannot exceed the requested input length. The collection
        // allowance admits only input slots, one FAT id, the FAT words, one
        // directory visit key and one chain slot; directory materialization
        // would need additional collection items.
        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(file.len());
        let acquired_and_available_items = file.len() + 1 + sector_size / 4 + 1 + 1;
        policy.limits.max_collection_items =
            cadmpeg_core::decode::u64_from_index(acquired_and_available_items);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty context fits the input limit");
        let mut source = std::io::Cursor::new(file.as_slice());
        let prefix = read_detection_prefix(&ctx, &mut source, file.len())
            .expect("readiness stops after structural directory availability");

        assert_eq!(prefix, file);
        assert_eq!(source.position(), cadmpeg_core::decode::u64_from_index(file.len()));

        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(stage_peak);
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("fixture root fits the input limit");
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect("borrowed records fit the structural-stage allowance");
        assert_eq!(probe, CompoundPrefixProbe::DirectoryEvidence(vec!["Wide".into()]));
        drop(storage);
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn prefix_probe_returns_only_evidence_storage() {
        let file = fixture_v4();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("fixture root fits the input limit");
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect("temporary probe phases fit their live storage");
        assert!(matches!(&probe, CompoundPrefixProbe::DirectoryEvidence(paths)
            if paths.len() == 1 && paths[0].as_str() == "Wide"));

        // The path text uses four bytes. The amortized Vec<String> starts at
        // four slots, so the returned owner holds only those slots and text.
        let evidence_bytes = "Wide".len() + 4 * std::mem::size_of::<String>();
        let error = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "check CFB evidence owner",
            )
            .expect_err("returned evidence consumes its exact live bytes");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "check CFB evidence owner"
                && limit.used == cadmpeg_core::decode::u64_from_index(evidence_bytes)
                && limit.additional == policy.limits.max_materialized_bytes));
        drop((probe, storage));
    }

    #[test]
    fn snapshot_preserves_parent_ordinals_for_nested_storage_siblings() {
        const V4_SECTOR_SIZE: usize = 4096;
        let mut file = prefix_evidence_growth_fixture();
        put_u32(
            sector_mut_with_size(&mut file, V4_SECTOR_SIZE, 2),
            4,
            FREE_SECTOR,
        );
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service())
            .expect("nested storage fixture fits the service policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("nested storage fixture parses");
        let paths = snapshot
            .entries()
            .iter()
            .map(CompoundEntry::path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["Store", "Store/A", "Store/A/B", "Store/C"]
        );
    }

    #[test]
    fn compound_summary_refuses_before_classification() {
        let file = fixture();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("setup");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let called = std::cell::Cell::new(false);
        assert!(
            matches!(snapshot.container_entries(&ctx, |_| { called.set(true); ContainerRole::Stream }), Err(CodecError::ResourceLimit(limit)) if limit.operation == "CFB container summaries")
        );
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
            let stream = snapshot
                .stream(&ctx, "Small")
                .expect("lookup admission")
                .expect("empty stream exists");
            assert_eq!(stream.start_sector(), marker);
            assert_eq!(stream.logical_size(), 0);
            assert!(snapshot
                .open(&ctx, stream)
                .expect("empty stream opens")
                .window()
                .is_empty());
            let summary = snapshot
                .container_entries(&ctx, |_| ContainerRole::Stream)
                .expect("summary admission");
            let entry = summary
                .iter()
                .find(|entry| entry.name == "Small")
                .expect("stream summary");
            assert_eq!(entry.attributes["start_sector"], marker.to_string());
        }
    }

    #[test]
    fn rejects_a_partial_structural_sector() {
        let mut file = fixture();
        file.truncate(SECTOR_SIZE * 12 + 37);
        assert!(!snapshot_parses(&file));
    }


    #[test]
    fn fat_id_population_is_refused_before_exceeding_its_declaration() {
        let mut file = fixture();
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, 11);
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let error = with_context(&file, &policy, |ctx| parse_state(ctx, &file))
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
    fn prefix_probe_preserves_parent_ordinals_for_nested_storage_siblings() {
        let file = prefix_evidence_growth_fixture();
        let CompoundPrefixProbe::DirectoryEvidence(paths) = probe(&file) else {
            panic!("usable prefix")
        };
        let ordered_paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
        // C is queued with Store as parent before A's child B appends another
        // path. Its later path still uses Store's ordinal after names grows.
        assert_eq!(
            ordered_paths,
            vec!["Store", "Store/A", "Store/A/B", "Store/C"]
        );
    }

    #[test]
    fn prefix_probe_does_not_charge_a_full_input_scan() {
        let file = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(file.len() - 1)
            .expect("synthetic prefix size fits work units");
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect("the probe charges its actual structural operations");
        assert!(matches!(
            probe,
            CompoundPrefixProbe::DirectoryEvidence(paths)
                if paths.iter().any(|path| path == "Store/Large")
        ));
        drop(storage);
    }

    #[test]
    fn prefix_probe_does_not_charge_an_unvisited_difat_sector() {
        let mut file = fixture();
        put_u32(&mut file, 68, 12);
        put_u32(&mut file, 72, 1);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One dynamic DIFAT-sector visit precedes the absent-sector check. The
        // probe does not scan that sector's 512 bytes.
        policy.limits.max_work_units = 1;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect("absent DIFAT sector returns incomplete");
        assert_eq!(probe, CompoundPrefixProbe::Incomplete);
        drop(storage);
    }

    #[test]
    fn prefix_probe_charges_each_difat_entry_visit() {
        let mut file = fixture();
        file.resize(file.len() + SECTOR_SIZE, 0xff);
        put_u32(&mut file, 68, 12);
        put_u32(&mut file, 72, 1);
        put_u32(sector_mut(&mut file, 12), 4, 11);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The sector step uses one unit, and inserting the first id into an
        // empty CFB B-tree set takes three node passes. The first entry visit
        // then fits; the second must refuse.
        let id_node_bytes = 11 * std::mem::size_of::<u32>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
        let work_limit = u64::try_from(1 + 3 * id_node_bytes + 1)
            .expect("DIFAT work total fits u64");
        policy.limits.max_work_units = work_limit;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let error = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect_err("second DIFAT entry visit exceeds the exact budget");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "visit CFB DIFAT entries"
                    && limit.used == work_limit
                    && limit.additional == 1
        ));
    }

    #[test]
    fn prefix_probe_charges_fat_words_without_a_byte_precharge() {
        let mut file = fixture();
        put_u32(sector_mut(&mut file, 11), 11 * 4, END_OF_CHAIN);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The path visits one FAT id, 128 FAT words, and one role-check item
        // before returning the specified malformed result.
        policy.limits.max_work_units = u64::try_from(1 + SECTOR_SIZE / 4 + 1)
            .expect("work total fits u64");
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect("the full FAT word traversal is admitted");
        assert_eq!(
            probe,
            CompoundPrefixProbe::Malformed(
                "CFB allocation sector has the wrong role marker".into()
            )
        );
        drop(storage);
    }

    #[test]
    fn prefix_probe_keeps_only_borrowed_chain_and_parsed_directory_storage() {
        let mut file = fixture();
        initialize_empty_directory_entries(sector_mut(&mut file, 0));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // FAT loading holds one id and the FAT words. Directory traversal
        // overlaps FAT words, one visited-id node and the chain capacity.
        // Parsing borrows sectors and overlaps only the chain and slots.
        let parsed_directory_bytes = 4 * std::mem::size_of::<DirectorySlot>();
        let id_node_bytes = 11 * std::mem::size_of::<u32>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<u32>().max(std::mem::align_of::<usize>());
        let fat_loading_peak = std::mem::size_of::<u32>()
            + (SECTOR_SIZE / 4) * std::mem::size_of::<u32>();
        let directory_chain_peak = (SECTOR_SIZE / 4) * std::mem::size_of::<u32>()
            + 4 * std::mem::size_of::<u32>()
            + id_node_bytes;
        let parse_peak = 4 * std::mem::size_of::<u32>() + parsed_directory_bytes;
        let exact_peak = fat_loading_peak.max(directory_chain_peak).max(parse_peak);
        assert_eq!(exact_peak, directory_chain_peak);
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(exact_peak);
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let (probe, storage) = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect("borrowed parsing fits the structural traversal peak");
        assert_eq!(
            probe,
            CompoundPrefixProbe::Malformed(
                "malformed container: invalid CFB root directory entry".into()
            )
        );
        drop(storage);

        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(exact_peak - 1);
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &policy).expect("fixture root");
        let error = CompoundPrefixProbe::inspect_with_context(&ctx, root)
            .expect_err("one byte below the directory-chain overlap refuses");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "CFB probe directory chain"
                && limit.used == cadmpeg_core::decode::u64_from_index((SECTOR_SIZE / 4) * std::mem::size_of::<u32>() + id_node_bytes)
                && limit.additional == cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<u32>())));
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
    fn prefix_probe_rejects_undiscovered_fat_count_without_declared_allocation() {
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
            Ok((CompoundPrefixProbe::Malformed(message), _))
                if message == "CFB DIFAT does not match its declared FAT count"
        ));
        assert_eq!(ctx.resource_refusal(), None);
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
        assert!(snapshot
            .stream(&ctx, "Store/Large")
            .expect("lookup admission")
            .is_some());
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
                .open(
                    &ctx,
                    snapshot
                        .stream(&ctx, "Wide")
                        .expect("lookup admission")
                        .expect("stream exists")
                )
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
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("setup");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let CodecError::ResourceLimit(first) =
            snapshot.entry(&ctx, "Store").expect_err("lookup work")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first.operation, "scan CFB path key");
        let CodecError::ResourceLimit(repeated) =
            snapshot.stream(&ctx, "Small").expect_err("fused lookup")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        policy.limits.max_work_units = DecodePolicy::service().limits.max_work_units;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(
            matches!(snapshot.entry(&ctx, "Store"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
        );
    }

    #[test]
    fn directory_name_queries_refuse_before_scans() {
        let file = fixture();
        let arena = DecodeArena::new();
        let (setup, _) =
            DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service()).expect("setup");
        let directory =
            parse_directory(&setup, &file[512..1024], CompoundVersion::V3).expect("directory");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) =
            cfb_name_cmp(&ctx, "alpha", "ALPHA").expect_err("comparison work")
        else {
            panic!("refusal")
        };
        let CodecError::ResourceLimit(repeated) =
            super::DirectoryName::new(&ctx, "alpha".into()).expect_err("fused name work")
        else {
            panic!("refusal")
        };
        assert_eq!(first, repeated);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(
            matches!(super::validate_root(&ctx, &directory), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan CFB root entries")
        );
    }

    #[test]
    fn directory_keys_use_length_preserving_simple_uppercase_units() {
        with_context(&[], &DecodePolicy::service(), |ctx| {
            assert_eq!(
                cfb_name_cmp(ctx, "alpha", "ALPHA").expect("comparison"),
                Ordering::Equal
            );
            assert_eq!(
                path_key(ctx, "Store/alpha").expect("key"),
                path_key(ctx, "store/ALPHA").expect("key")
            );
            assert_ne!(
                path_key(ctx, "ß").expect("key"),
                path_key(ctx, "SS").expect("key")
            );
            assert_eq!(
                cfb_name_cmp(ctx, "ᾠ", "ᾨ").expect("simple uppercase comparison"),
                Ordering::Equal
            );
            assert_eq!(
                path_key(ctx, "ᾠ").expect("key"),
                path_key(ctx, "ᾨ").expect("key")
            );
        });
        assert_eq!(cfb_upper_unit(0xd800), 0xd800);
    }

    #[test]
    fn cfb_upper_unit_maps_every_expansion_with_a_distinct_simple_target() {
        for (first, last) in [(0x1f80, 0x1f87), (0x1f90, 0x1f97), (0x1fa0, 0x1fa7)] {
            for unit in first..=last {
                assert_eq!(cfb_upper_unit(unit), unit + 8, "U+{unit:04X}");
            }
        }
        for (unit, uppercase) in [(0x1fb3, 0x1fbc), (0x1fc3, 0x1fcc), (0x1ff3, 0x1ffc)] {
            assert_eq!(cfb_upper_unit(unit), uppercase, "U+{unit:04X}");
        }
    }

    #[test]
    fn stream_lookup_uses_simple_uppercase_for_full_expansion() {
        let mut file = fixture();
        sector_mut(&mut file, 0)[3 * 128..4 * 128].fill(0);
        directory_entry(
            sector_mut(&mut file, 0),
            3,
            "ᾠ",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            2,
            4096,
        );
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&file, &arena, &DecodePolicy::service())
                .expect("synthetic CFB fits the service policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic CFB parses");
        let stream = snapshot
            .stream(&ctx, "Store/ᾨ")
            .expect("lookup admission")
            .expect("simple-uppercase stream lookup succeeds");
        assert_eq!(stream.path(), "Store/ᾠ");
        assert_eq!(stream.logical_size(), 4096);
    }

    #[test]
    fn sibling_tree_rejects_names_equal_under_simple_uppercase() {
        let mut directory = [0_u8; 3 * 128];
        initialize_empty_directory_entries(&mut directory);
        directory_entry(
            &mut directory,
            1,
            "ᾠ",
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
            "ᾨ",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        let error = with_context(&directory, &DecodePolicy::service(), |ctx| {
            let entries = parse_directory(ctx, &directory, CompoundVersion::V3)?;
            validate_sibling_tree(ctx, &entries, 1)
        })
        .expect_err("siblings with the same simple uppercase key are invalid");
        assert!(matches!(
            error,
            CodecError::Malformed(message)
                if message == "CFB sibling tree violates directory-name ordering"
        ));
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
            // One record visit, two two-byte UTF-16 scans, three output bytes,
            // and one character plus the end probe.
            policy.limits.max_work_units = 1 + 2 * 2 + 3 + 2;
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
    fn rejects_stale_unallocated_directory_fields() {
        let mut directory = vec![0_u8; 128];
        directory[68..80].fill(0xff);
        directory[8] = 1;
        let error = with_context(&directory, &DecodePolicy::service(), |ctx| {
            parse_directory(ctx, &directory, CompoundVersion::V3)
        })
        .expect_err("free entries allow only NOSTREAM pointers");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "invalid CFB unallocated directory entry"));
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
                if kind == 1 { 0 } else { END_OF_CHAIN },
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
        assert!(with_context(&[], &DecodePolicy::service(), |ctx| {
            super::DirectoryName::new(ctx, String::new())
        })
        .is_err());
        assert_eq!(
            with_context(&[], &DecodePolicy::service(), |ctx| {
                super::DirectoryName::new(ctx, " ".into())
            })
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
                .stream(&ctx, "Small")
                .expect("lookup admission")
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
        policy.limits.max_materialized_bytes = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&file, &arena, &policy)
            .expect("root input fits its independent budget");
        let CodecError::ResourceLimit(limit) =
            CompoundSnapshot::new(&ctx, root).expect_err("scoped metadata exceeds one byte")
        else {
            panic!("scoped budget refusal is typed")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
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

    fn prefix_evidence_growth_fixture() -> Vec<u8> {
        const V4_SECTOR_SIZE: usize = 4096;
        let mut file = fixture_v4();
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
            directory,
            1,
            "Store",
            1,
            NO_STREAM,
            NO_STREAM,
            2,
            0,
            0,
        );
        directory_entry(
            directory,
            2,
            "A",
            1,
            NO_STREAM,
            3,
            4,
            0,
            0,
        );
        directory_entry(
            directory,
            3,
            "C",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
        directory[3 * directory_layout::LEN + directory_layout::COLOR] = 0;
        directory_entry(
            directory,
            4,
            "B",
            2,
            NO_STREAM,
            NO_STREAM,
            NO_STREAM,
            END_OF_CHAIN,
            0,
        );
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
        let entry = &mut directory[index * directory_layout::LEN..(index + 1) * directory_layout::LEN];
        let encoded = name.encode_utf16().collect::<Vec<_>>();
        for (offset, unit) in encoded.iter().enumerate() {
            put_u16(entry, directory_layout::NAME + offset * 2, *unit);
        }
        put_u16(
            entry,
            directory_layout::NAME_LENGTH,
            u16::try_from((encoded.len() + 1) * 2).expect("test name length fits u16"),
        );
        entry[directory_layout::OBJECT_TYPE] = object_type;
        entry[directory_layout::COLOR] = 1;
        put_u32(entry, directory_layout::LEFT, left);
        put_u32(entry, directory_layout::RIGHT, right);
        put_u32(entry, directory_layout::CHILD, child);
        put_u32(entry, directory_layout::START_SECTOR, start_sector);
        entry[directory_layout::STREAM_SIZE..directory_layout::LEN].copy_from_slice(&size.to_le_bytes());
    }

    fn sector_mut(file: &mut [u8], id: usize) -> &mut [u8] {
        sector_mut_with_size(file, SECTOR_SIZE, id)
    }

    fn sector_mut_with_size(file: &mut [u8], sector_size: usize, id: usize) -> &mut [u8] {
        let start = sector_size * (id + 1);
        &mut file[start..start + sector_size]
    }

    fn initialize_empty_directory_entries(directory: &mut [u8]) {
        for entry in directory.chunks_exact_mut(directory_layout::LEN) {
            entry.fill(0);
            entry[directory_layout::LEFT..directory_layout::CLSID].fill(0xff);
        }
    }

    fn snapshot_parses(file: &[u8]) -> bool {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let Ok((ctx, root)) = DecodeContext::from_root_bytes(file, &arena, &policy) else {
            return false;
        };
        let snapshot = CompoundSnapshot::new(&ctx, root);
        snapshot.is_ok()
    }
}
