// SPDX-License-Identifier: Apache-2.0
//! Outer `.sldprt` container scanning and inspection.
//!
//! Files start with an 8-byte `file_id` and big-endian version header. A shared
//! marker introduces raw-DEFLATE blocks, cache cells, and tail-directory
//! entries. [`scan`] classifies marker occurrences with structure-specific
//! invariants, validates block CRC-32 values, inflates payloads, decodes stored
//! section names, and extracts embedded Parasolid streams.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::compound::{CompoundEntry, CompoundPrefixProbe, CompoundSnapshot};
use cadmpeg_container::compression::{inflate_bounded_probe, inflate_deflate, inflate_zlib_member};
use cadmpeg_core::bytes::{contains, find};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, View};
use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::hash::sha256_hex;

use crate::layout::block_frame_header as block_hdr;
use crate::layout::cache_cell_header as cache_hdr;
use crate::layout::outer_header as outer_hdr;
use crate::layout::tail_directory_entry as dir_ent;
use crate::layout::zlb_wrapper_header as zlb_hdr;

/// Marker shared by block, cache-cell, and directory frames.
pub const MARKER: [u8; 6] = block_hdr::MARKER_VALUE;

/// Upper bound on a single decompressed block, guarding a corrupt `uncomp_sz`
/// from driving an unbounded allocation. Real part streams sit far below this.
const MAX_UNCOMP: usize = 512 * 1024 * 1024;

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// A CRC-validated compressed block (payload family in `attributes`).
    pub const BLOCK: &str = "block";
    /// A tail section-directory entry naming one OPC part.
    pub const DIRECTORY_ENTRY: &str = "directory-entry";
    /// A cache-cell section-index grid entry (not a compressed payload).
    pub const CACHE_CELL: &str = "cache-cell";
    /// A named stream in a Compound File Binary container.
    pub const COMPOUND_STREAM: &str = "compound-stream";
}

/// Classify a decompressed block payload by signature.
///
/// The returned labels form the `family` values exposed by [`Block`] and
/// [`summarize`]. Unknown signatures return `"unknown"`.
pub fn payload_family(payload: &[u8]) -> &'static str {
    if payload.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        "png-preview"
    } else if is_bmp_thumbnail(payload) {
        "bmp-thumbnail"
    } else if payload.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        "ole2"
    } else if contains(payload, b"uoTempBodyTessData_c")
        || contains(payload, b"uoTempFaceTessData_c")
    {
        "tessellation"
    } else if payload.starts_with(&[0xff, 0xff, 0x01, 0x00]) {
        "sw-objects"
    } else if payload.starts_with(b"unqlite") {
        "unqlite"
    } else if payload.starts_with(b"<?xml")
        || payload.starts_with(&[0xff, 0xfe])
        || (payload.first() == Some(&0x86) && contains(&payload[..payload.len().min(64)], b"<"))
    {
        "xml"
    } else {
        "unknown"
    }
}

fn is_bmp_thumbnail(payload: &[u8]) -> bool {
    let Some(header_size) = View::u32_le_at(payload, 4) else {
        return false;
    };
    let Some(bits_per_pixel) = View::u16_le_at(payload, 18) else {
        return false;
    };
    header_size == 40 && matches!(bits_per_pixel, 1 | 4 | 8 | 16 | 24 | 32)
}

/// Find a Parasolid `PS\0\0` signature in the first 64 payload bytes.
pub fn parasolid_offset(payload: &[u8]) -> Option<usize> {
    const SIG: &[u8] = &[b'P', b'S', 0x00, 0x00];
    let window = payload.len().min(64);
    find(&payload[..window], SIG)
}

/// Decode a nibble-swapped section name.
///
/// Returns `None` when any decoded byte falls outside printable ASCII.
pub fn nibble_swap_name(raw: &[u8]) -> Option<String> {
    let mut s = String::with_capacity(raw.len());
    for &b in raw {
        let swapped = b.rotate_left(4);
        if !(0x20..0x7f).contains(&swapped) {
            return None;
        }
        s.push(swapped as char);
    }
    Some(s)
}

/// One validated compressed block.
#[derive(Debug, Clone)]
pub struct Block {
    /// Byte offset of the marker in the file.
    pub offset: usize,
    /// Frame `type_id`.
    pub type_id: u32,
    /// Compressed payload length.
    pub comp_sz: u32,
    /// Declared decompressed length, equal to `payload.len()`.
    pub uncomp_sz: u32,
    /// OPC section name decoded from the preamble, when printable.
    pub section: Option<String>,
    /// Payload-family label from [`payload_family`], or `"parasolid"`.
    pub family: &'static str,
    /// The decompressed payload bytes.
    pub payload: Vec<u8>,
    /// First direct or nested Parasolid stream in this block.
    pub ps_stream: Option<Vec<u8>>,
    /// Every Parasolid stream carried by this block.
    pub ps_streams: Vec<Vec<u8>>,
    /// Outer-payload offset of each entry in `ps_streams`.
    pub ps_stream_offsets: Vec<usize>,
}

/// One tail-directory entry naming a section.
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    /// Byte offset of the marker.
    pub offset: usize,
    /// Frame `type_id`.
    pub type_id: u32,
    /// The section's stored/uncompressed size.
    pub size: u32,
    /// Decoded section name.
    pub name: String,
    /// Per-entry descriptor bytes at frame offset +26.
    pub descriptor: [u8; 14],
    /// File-level directory trailer following the encoded name.
    pub trailer: [u8; 6],
}

/// One cache-cell section-index entry.
#[derive(Debug, Clone)]
pub struct CacheCell {
    /// Byte offset of the marker.
    pub offset: usize,
    /// The logical cell size `L`.
    pub logical_len: u32,
    /// Decoded section name.
    pub name: String,
}

/// One named stream in a Compound File Binary container.
#[derive(Debug, Clone)]
pub struct CompoundStream {
    /// Storage-qualified stream path.
    pub path: String,
    /// Unique directory entry identifier.
    pub directory_id: u32,
    /// First regular or mini sector identifier.
    pub start_sector: u32,
    /// Exact stream bytes.
    pub payload: Vec<u8>,
    /// Inflated semantic bytes when the stream uses the `__ZLB` wrapper.
    pub decoded_payload: Option<Vec<u8>>,
    /// Every Parasolid stream carried by this compound stream.
    pub ps_streams: Vec<Vec<u8>>,
    /// Raw compound-stream offset of each entry in `ps_streams`.
    pub ps_stream_offsets: Vec<usize>,
}

/// Complete result of an outer-container scan.
pub struct ContainerScan<'a> {
    /// Complete source image for exact passthrough writing.
    pub source_image: &'a [u8],
    /// Big-endian outer version word.
    pub version: u32,
    /// CRC-validated compressed blocks, in file order.
    pub blocks: Vec<Block>,
    /// Tail directory entries, in file order.
    pub directory: Vec<DirectoryEntry>,
    /// Cache-cell grid entries, in file order.
    pub cache_cells: Vec<CacheCell>,
    /// Named streams when the source uses the Compound File Binary envelope.
    pub compound_streams: Vec<CompoundStream>,
}

#[derive(Clone, Copy)]
pub(crate) enum Section<'a> {
    Block(&'a Block),
    Compound(&'a CompoundStream),
}

impl<'a> Section<'a> {
    pub(crate) fn name(self) -> Option<&'a str> {
        match self {
            Self::Block(block) => block.section.as_deref(),
            Self::Compound(stream) => Some(&stream.path),
        }
    }

    pub(crate) fn display_name(self) -> String {
        self.name().map_or_else(
            || match self {
                Self::Block(block) => format!("block@{}", block.offset),
                Self::Compound(_) => unreachable!("compound streams are named"),
            },
            str::to_string,
        )
    }

    pub(crate) fn ordinal(self) -> usize {
        match self {
            Self::Block(block) => block.offset,
            Self::Compound(stream) => stream.directory_id as usize,
        }
    }

    pub(crate) fn native_id(self) -> String {
        match self {
            Self::Block(block) => format!("sldprt:file:block#{}", block.offset),
            Self::Compound(stream) => {
                format!("sldprt:file:compound-stream#{}", stream.directory_id)
            }
        }
    }

    pub(crate) fn payload(self) -> &'a [u8] {
        match self {
            Self::Block(block) => &block.payload,
            Self::Compound(stream) => stream.decoded_payload.as_deref().unwrap_or(&stream.payload),
        }
    }

    pub(crate) fn ps_streams(self) -> &'a [Vec<u8>] {
        match self {
            Self::Block(block) => &block.ps_streams,
            Self::Compound(stream) => &stream.ps_streams,
        }
    }

    pub(crate) fn ps_stream_offsets(self) -> &'a [usize] {
        match self {
            Self::Block(block) => &block.ps_stream_offsets,
            Self::Compound(stream) => &stream.ps_stream_offsets,
        }
    }
}

impl ContainerScan<'_> {
    pub(crate) fn sections(&self) -> impl Iterator<Item = Section<'_>> {
        self.blocks
            .iter()
            .map(Section::Block)
            .chain(self.compound_streams.iter().map(Section::Compound))
    }
}

const COMPOUND_FILE_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const WRAPPED_PAYLOAD_MAGIC: [u8; 16] = zlb_hdr::MAGIC_VALUE;

/// Test whether a prefix contains the container marker after its outer header.
///
/// This structural check does not validate block framing or CRC-32.
pub fn looks_like_sldprt(prefix: &[u8]) -> bool {
    if prefix.starts_with(&COMPOUND_FILE_MAGIC) {
        return CompoundPrefixProbe::inspect(prefix)
            .paths()
            .is_some_and(|paths| {
                paths.iter().any(|path| {
                    path.rsplit('/')
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case("ISolidWorksInformation"))
                })
            });
    }
    if prefix.len() < outer_hdr::LEN + MARKER.len() {
        return false;
    }
    prefix[outer_hdr::LEN..]
        .windows(MARKER.len())
        .any(|w| w == MARKER)
}

/// Test whether a prefix has the generic Compound File Binary signature.
pub fn looks_like_compound_file(prefix: &[u8]) -> bool {
    prefix.starts_with(&COMPOUND_FILE_MAGIC)
}

/// Scan an in-memory `.sldprt` image.
///
/// Truncated input produces a scan containing every structure that could be
/// validated; missing outer-header bytes yield version zero.
pub fn scan_bytes(bytes: &[u8]) -> ContainerScan<'_> {
    if bytes.starts_with(&COMPOUND_FILE_MAGIC) {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let compound_streams = DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .ok()
            .and_then(|(ctx, root)| compound_streams(&ctx, root).ok())
            .unwrap_or_default();
        return ContainerScan {
            source_image: bytes,
            version: 0,
            blocks: Vec::new(),
            directory: Vec::new(),
            cache_cells: Vec::new(),
            compound_streams,
        };
    }
    let version = native_version(bytes);
    let (blocks, directory, cache_cells) = match walk_native_markers(bytes, |off| {
        Ok::<_, std::convert::Infallible>(try_block(bytes, off))
    }) {
        Ok(frames) => frames,
        Err(never) => match never {},
    };

    ContainerScan {
        source_image: bytes,
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams: Vec::new(),
    }
}

fn native_version(bytes: &[u8]) -> u32 {
    let mut view = View::over_retained(bytes);
    view.seek(outer_hdr::VERSION)
        .and_then(|()| view.u32_be())
        .unwrap_or(0)
}

/// Every marker hit is tried as a block first (the CRC gate is effectively
/// false-positive-free), then as a cache cell, then as a directory entry.
type NativeWalk<E> = Result<(Vec<Block>, Vec<DirectoryEntry>, Vec<CacheCell>), E>;

fn walk_native_markers<E>(
    bytes: &[u8],
    mut try_one_block: impl FnMut(usize) -> Result<Option<RawBlock>, E>,
) -> NativeWalk<E> {
    let mut blocks = Vec::new();
    let mut directory = Vec::new();
    let mut cache_cells = Vec::new();
    let mut i = outer_hdr::LEN;
    while i + MARKER.len() <= bytes.len() {
        if bytes[i..i + MARKER.len()] != MARKER {
            i += 1;
            continue;
        }
        if let Some(block) = try_one_block(i)? {
            i = block.offset + block_hdr::LEN + block.preamble_len + block.comp_sz as usize;
            blocks.push(block.into_block());
            continue;
        }
        if let Some(cell) = try_cache_cell(bytes, i) {
            cache_cells.push(cell);
        } else if let Some(entry) = try_directory_entry(bytes, i) {
            directory.push(entry);
        }
        i += 1;
    }
    Ok((blocks, directory, cache_cells))
}

fn compound_stream(
    path: String,
    directory_id: u32,
    start_sector: u32,
    bytes: Vec<u8>,
    decoded_bytes: Option<Vec<u8>>,
) -> CompoundStream {
    let located_streams = crate::parasolid::extract_streams_with_offsets(&bytes);
    let ps_stream_offsets = located_streams.iter().map(|(offset, _)| *offset).collect();
    let ps_streams = located_streams
        .into_iter()
        .map(|(_, payload)| payload)
        .collect();
    CompoundStream {
        path,
        directory_id,
        start_sector,
        payload: bytes,
        decoded_payload: decoded_bytes,
        ps_streams,
        ps_stream_offsets,
    }
}

/// Scans an in-memory image while routing inflate through the decode budget.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<ContainerScan<'a>, CodecError> {
    if root.window().starts_with(&COMPOUND_FILE_MAGIC) {
        let compound_streams = compound_streams(ctx, root)?;
        return Ok(ContainerScan {
            source_image: root.window(),
            version: 0,
            blocks: Vec::new(),
            directory: Vec::new(),
            cache_cells: Vec::new(),
            compound_streams,
        });
    }
    let bytes = root.window();
    let version = native_version(bytes);
    let (blocks, directory, cache_cells) =
        walk_native_markers(bytes, |off| try_block_budgeted(ctx, root, off))?;
    Ok(ContainerScan {
        source_image: bytes,
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams: Vec::new(),
    })
}

/// CFB directory/FAT/open is [`CompoundSnapshot`]; ZLB unwrap and Parasolid
/// extract stay codec-local because they are `SolidWorks` payload semantics, not
/// CFB.
fn compound_streams<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<Vec<CompoundStream>, CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    snapshot
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            CompoundEntry::Stream(stream) => Some(stream),
            CompoundEntry::Storage(_) => None,
        })
        .map(|entry| {
            let view = snapshot.open(ctx, entry)?;
            let payload = ctx.copy_retained(
                view.window(),
                "retain SolidWorks CFB stream",
                Some(view.location()),
            )?;
            let decoded = decode_wrapped_payload_budgeted(ctx, view)?;
            Ok(compound_stream(
                entry.path().to_owned(),
                entry.id().directory_id(),
                entry.start_sector(),
                payload,
                decoded,
            ))
        })
        .collect()
}

fn decode_wrapped_payload_budgeted<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let payload = source.window();
    if payload.get(..WRAPPED_PAYLOAD_MAGIC.len()) != Some(&WRAPPED_PAYLOAD_MAGIC) {
        return Ok(None);
    }
    let Some(uncompressed_size) =
        View::u32_le_at(payload, zlb_hdr::UNCOMPRESSED_SIZE).map(u64::from)
    else {
        return Ok(None);
    };
    let Some(compressed_size) = View::u32_le_at(payload, zlb_hdr::ZLIB_MEMBER_SIZE)
        .and_then(|size| usize::try_from(size).ok())
    else {
        return Ok(None);
    };
    if uncompressed_size == 0 || compressed_size == 0 {
        return Ok(None);
    }
    let Some(member_end) = zlb_hdr::LEN.checked_add(compressed_size) else {
        return Ok(None);
    };
    if payload.get(zlb_hdr::LEN..member_end).is_none() {
        return Ok(None);
    }
    let Some(member) = source.child(source.start() + zlb_hdr::LEN, source.start() + member_end)
    else {
        return Ok(None);
    };
    let (decoded, consumed) =
        match inflate_zlib_member(ctx, member, ExpandSpec::Exact(uncompressed_size)) {
            Ok(result) => result,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(None),
        };
    if consumed != compressed_size {
        return Ok(None);
    }
    ctx.copy_retained(
        decoded.window(),
        "retain decoded SolidWorks CFB stream",
        Some(source.location()),
    )
    .map(Some)
}

/// A block plus the preamble length needed to advance past it.
struct RawBlock {
    offset: usize,
    type_id: u32,
    comp_sz: u32,
    uncomp_sz: u32,
    preamble_len: usize,
    section: Option<String>,
    family: &'static str,
    payload: Vec<u8>,
    ps_stream: Option<Vec<u8>>,
    ps_streams: Vec<Vec<u8>>,
    ps_stream_offsets: Vec<usize>,
}

impl RawBlock {
    fn into_block(self) -> Block {
        Block {
            offset: self.offset,
            type_id: self.type_id,
            comp_sz: self.comp_sz,
            uncomp_sz: self.uncomp_sz,
            section: self.section,
            family: self.family,
            payload: self.payload,
            ps_stream: self.ps_stream,
            ps_streams: self.ps_streams,
            ps_stream_offsets: self.ps_stream_offsets,
        }
    }
}

#[derive(Clone, Copy)]
struct BlockFrame {
    type_id: u32,
    crc: u32,
    comp_sz: u32,
    uncomp_sz: u32,
    pre_sz: u32,
}

fn read_block_frame(bytes: &[u8], off: usize) -> Option<(BlockFrame, usize, usize)> {
    let type_id = View::u32_le_at(bytes, off + block_hdr::TYPE_ID)?;
    let crc = View::u32_le_at(bytes, off + block_hdr::CRC32)?;
    let comp_sz = View::u32_le_at(bytes, off + block_hdr::COMP_SZ)?;
    let uncomp_sz = View::u32_le_at(bytes, off + block_hdr::UNCOMP_SZ)?;
    let pre_sz = View::u32_le_at(bytes, off + block_hdr::PRE_SZ)?;

    let comp = comp_sz as usize;
    let pre = pre_sz as usize;
    let uncomp = uncomp_sz as usize;
    if comp == 0 || uncomp == 0 || uncomp > MAX_UNCOMP {
        return None;
    }
    let payload_start = off + block_hdr::LEN + pre;
    let payload_end = payload_start.checked_add(comp)?;
    let _ = bytes.get(payload_start..payload_end)?;
    Some((
        BlockFrame {
            type_id,
            crc,
            comp_sz,
            uncomp_sz,
            pre_sz,
        },
        payload_start,
        payload_end,
    ))
}

fn block_from_inflated(
    bytes: &[u8],
    off: usize,
    frame: &BlockFrame,
    inflated: Vec<u8>,
) -> Option<RawBlock> {
    if inflated.len() != frame.uncomp_sz as usize {
        return None;
    }
    if crc32fast::hash(&inflated) != frame.crc {
        return None;
    }

    let payload_start = off + block_hdr::LEN + frame.pre_sz as usize;
    let preamble = bytes
        .get(off + block_hdr::LEN..payload_start)
        .unwrap_or(&[]);
    let section = nibble_swap_name(preamble);
    // A Parasolid block is one from which a `PS\0\0` stream can be extracted (in
    // plain, wrapped, or nested form); otherwise fall back to a byte-signature
    // family label.
    let located_streams = crate::parasolid::extract_streams_with_offsets(&inflated);
    let ps_stream_offsets = located_streams.iter().map(|(offset, _)| *offset).collect();
    let ps_streams = located_streams
        .into_iter()
        .map(|(_, stream)| stream)
        .collect::<Vec<_>>();
    let ps_stream = ps_streams.first().cloned();
    let family = if ps_streams.is_empty() {
        payload_family(&inflated)
    } else {
        "parasolid"
    };

    Some(RawBlock {
        offset: off,
        type_id: frame.type_id,
        comp_sz: frame.comp_sz,
        uncomp_sz: frame.uncomp_sz,
        preamble_len: frame.pre_sz as usize,
        section,
        family,
        payload: inflated,
        ps_stream,
        ps_streams,
        ps_stream_offsets,
    })
}

fn try_block(bytes: &[u8], off: usize) -> Option<RawBlock> {
    let (frame, payload_start, payload_end) = read_block_frame(bytes, off)?;
    let payload = bytes.get(payload_start..payload_end)?;
    let inflated = inflate_bounded_probe(payload, frame.uncomp_sz as usize)?;
    block_from_inflated(bytes, off, &frame, inflated)
}

fn try_block_budgeted<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    off: usize,
) -> Result<Option<RawBlock>, CodecError> {
    let bytes = root.window();
    let Some((frame, payload_start, payload_end)) = read_block_frame(bytes, off) else {
        return Ok(None);
    };
    let Some(abs_start) = root.start().checked_add(payload_start) else {
        return Ok(None);
    };
    let Some(abs_end) = root.start().checked_add(payload_end) else {
        return Ok(None);
    };
    let Some(payload_view) = root.child(abs_start, abs_end) else {
        return Ok(None);
    };
    let inflated = match inflate_deflate(
        ctx,
        payload_view,
        ExpandSpec::Exact(u64::from(frame.uncomp_sz)),
    ) {
        Ok(view) => view.window().to_vec(),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    Ok(block_from_inflated(bytes, off, &frame, inflated))
}

/// Test a marker hit against the cache-cell relational invariant
/// (`f@+10 == 2L`, `f@+14 == L/2`, `f@+18 == L`, `f@+22 == name_len`) plus a
/// printable nibble-swapped name ([spec §2.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#12-cache-cell-section-index-grid)).
fn try_cache_cell(bytes: &[u8], off: usize) -> Option<CacheCell> {
    let two_l = View::u32_le_at(bytes, off + cache_hdr::TWO_L)?;
    let half_l = View::u32_le_at(bytes, off + cache_hdr::HALF_L)?;
    let l = View::u32_le_at(bytes, off + cache_hdr::L)?;
    let name_len = View::u32_le_at(bytes, off + cache_hdr::NAME_LEN)?;

    if l == 0 || two_l != l.wrapping_mul(2) || half_l != l / 2 {
        return None;
    }
    if name_len == 0 || name_len >= 500 {
        return None;
    }
    let name_start = off + cache_hdr::LEN;
    let raw = bytes.get(name_start..name_start + name_len as usize)?;
    let name = nibble_swap_name(raw)?;
    Some(CacheCell {
        offset: off,
        logical_len: l,
        name,
    })
}

/// Test a marker hit against the tail-directory frame: two zero words at +10 and
/// +18, a size at +14, a name length at +22, a 14-byte descriptor, then a
/// printable nibble-swapped name ([spec §2.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#13-tail-section-directory)).
fn try_directory_entry(bytes: &[u8], off: usize) -> Option<DirectoryEntry> {
    let type_id = View::u32_le_at(bytes, off + dir_ent::TYPE_ID)?;
    let zero_a = View::u32_le_at(bytes, off + dir_ent::ZERO_AT_10)?;
    let size = View::u32_le_at(bytes, off + dir_ent::SIZE)?;
    let zero_b = View::u32_le_at(bytes, off + dir_ent::ZERO_AT_18)?;
    let name_len = View::u32_le_at(bytes, off + dir_ent::NAME_LEN)?;
    if zero_a != 0 || zero_b != 0 {
        return None;
    }
    if name_len == 0 || name_len >= 500 {
        return None;
    }
    let name_start = off + dir_ent::LEN;
    let raw = bytes.get(name_start..name_start + name_len as usize)?;
    let name = nibble_swap_name(raw)?;
    let descriptor = bytes
        .get(off + dir_ent::DESCRIPTOR..off + dir_ent::LEN)?
        .try_into()
        .ok()?;
    let trailer = bytes
        .get(name_start + name_len as usize..name_start + name_len as usize + 6)?
        .try_into()
        .ok()?;
    Some(DirectoryEntry {
        offset: off,
        type_id,
        size,
        name,
        descriptor,
        trailer,
    })
}

/// Convert a scan into the generic container inventory returned by
/// [`cadmpeg_ir::Codec::inspect`].
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let mut entries = Vec::new();

    for b in &scan.blocks {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), b.offset.to_string());
        attributes.insert("type_id".to_string(), format!("0x{:08x}", b.type_id));
        attributes.insert("family".to_string(), b.family.to_string());
        attributes.insert("sha256".to_string(), sha256_hex(&b.payload));
        if let Some(ps) = &b.ps_stream {
            if let Some(sch) = crate::parasolid::stream_header(ps) {
                attributes.insert("parasolid_schema".to_string(), sch.schema.clone());
                attributes.insert("parasolid_description".to_string(), sch.description.clone());
            }
        }
        entries.push(ContainerEntry {
            name: b
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", b.offset)),
            role: role::BLOCK.to_string(),
            compression: "deflate".to_string(),
            compressed_size: b.comp_sz as u64,
            uncompressed_size: b.uncomp_sz as u64,
            attributes,
        });
    }

    for d in &scan.directory {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), d.offset.to_string());
        attributes.insert("type_id".to_string(), format!("0x{:08x}", d.type_id));
        entries.push(ContainerEntry {
            name: d.name.clone(),
            role: role::DIRECTORY_ENTRY.to_string(),
            compression: "none".to_string(),
            compressed_size: 0,
            uncompressed_size: d.size as u64,
            attributes,
        });
    }

    for c in &scan.cache_cells {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), c.offset.to_string());
        attributes.insert("logical_len".to_string(), c.logical_len.to_string());
        entries.push(ContainerEntry {
            name: c.name.clone(),
            role: role::CACHE_CELL.to_string(),
            compression: "none".to_string(),
            compressed_size: 0,
            uncompressed_size: 0,
            attributes,
        });
    }

    for stream in &scan.compound_streams {
        let mut attributes = BTreeMap::new();
        attributes.insert("start_sector".to_string(), stream.start_sector.to_string());
        attributes.insert("sha256".to_string(), sha256_hex(&stream.payload));
        attributes.insert(
            "family".to_string(),
            payload_family(&stream.payload).to_string(),
        );
        entries.push(ContainerEntry {
            name: stream.path.clone(),
            role: role::COMPOUND_STREAM.to_string(),
            compression: "compound-file".to_string(),
            compressed_size: stream.payload.len() as u64,
            uncompressed_size: stream.payload.len() as u64,
            attributes,
        });
    }

    let mut notes = vec![format!(
        "outer version word: 0x{:08x}; {} CRC-validated block(s), {} tail-directory \
         entry/entries, {} cache-cell(s), {} compound stream(s)",
        scan.version,
        scan.blocks.len(),
        scan.directory.len(),
        scan.cache_cells.len(),
        scan.compound_streams.len()
    )];
    match active_parasolid_summary(scan) {
        Some((name, size, sch)) => notes.push(format!(
            "active Parasolid B-rep candidate: {} ({} bytes, schema {})",
            name, size, sch.schema
        )),
        None => notes.push(
            "no unique active Parasolid partition located; available B-rep sites remain decodable"
                .to_string(),
        ),
    }
    notes.push(
        "Parasolid body streams supply the typed topology and analytic carriers used by decode"
            .to_string(),
    );

    ContainerSummary::classified(
        crate::dialect::classify_layers(scan),
        if scan.compound_streams.is_empty() {
            "sldprt-blocks"
        } else {
            "compound-file-binary"
        },
        entries,
        notes,
    )
}

pub(crate) fn active_parasolid_summary(
    scan: &ContainerScan,
) -> Option<(String, usize, crate::parasolid::StreamHeader)> {
    let selected = select_active_parasolid_site(scan)?;
    Some((selected.name(), selected.payload.len(), selected.header))
}

/// Test whether either outer envelope carries a framed Parasolid body stream.
pub fn has_parasolid_body_stream(scan: &ContainerScan) -> bool {
    scan.blocks
        .iter()
        .flat_map(|block| &block.ps_streams)
        .chain(
            scan.compound_streams
                .iter()
                .flat_map(|stream| &stream.ps_streams),
        )
        .filter_map(|payload| crate::parasolid::stream_header(payload))
        .any(|header| crate::parasolid::is_body_stream(&header))
}

/// Select the unique Parasolid partition block for the active configuration.
///
/// An explicit active configuration index is authoritative. Without one, the
/// available body sites must contain exactly one non-ghost partition candidate.
/// This compatibility API returns only a block-envelope site; the decoder uses
/// [`select_active_parasolid_site`] so both envelopes retain their source site.
pub fn select_active_parasolid<'a>(
    scan: &'a ContainerScan<'_>,
) -> Option<(&'a Block, crate::parasolid::StreamHeader)> {
    let selected = select_active_parasolid_site(scan)?;
    match selected.origin {
        ActiveParasolidOrigin::Block(block) => Some((block, selected.header)),
        ActiveParasolidOrigin::Compound(_) => None,
    }
}

/// The source site of one selected Parasolid partition stream.
#[derive(Clone, Copy)]
pub(crate) enum ActiveParasolidOrigin<'a> {
    /// A native block-envelope block.
    Block(&'a Block),
    /// A Compound File Binary stream.
    Compound(&'a CompoundStream),
}

impl ActiveParasolidOrigin<'_> {
    fn site_key(self) -> String {
        match self {
            Self::Block(block) => format!("block@{}", block.offset),
            Self::Compound(stream) => format!("compound@{}", stream.directory_id),
        }
    }
}

/// One selected Parasolid partition stream and its source site.
pub(crate) struct ActiveParasolidSite<'a> {
    pub(crate) origin: ActiveParasolidOrigin<'a>,
    pub(crate) payload: &'a [u8],
    pub(crate) header: crate::parasolid::StreamHeader,
}

impl ActiveParasolidSite<'_> {
    pub(crate) fn name(&self) -> String {
        match self.origin {
            ActiveParasolidOrigin::Block(block) => block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            ActiveParasolidOrigin::Compound(stream) => stream.path.clone(),
        }
    }

    pub(crate) fn site_key(&self) -> String {
        self.origin.site_key()
    }
}

/// Select the unique Parasolid partition stream for the active configuration.
///
/// The selector is shared by native and Compound File Binary envelopes. A
/// body stream is admissible only when its source name and header identify it
/// as a non-ghost, non-deltas partition. A manifest or explicit source index
/// narrows the candidates; with no index exactly one candidate is required.
pub(crate) fn select_active_parasolid_site<'a>(
    scan: &'a ContainerScan<'_>,
) -> Option<ActiveParasolidSite<'a>> {
    let active_configuration = active_configuration_index(scan);
    let mut candidates = Vec::new();
    for block in &scan.blocks {
        let section = block.section.as_deref().unwrap_or("").to_ascii_lowercase();
        let section_is_partition = section.contains("partition")
            && !section.contains("ghost")
            && !section.contains("deltas")
            && !section.contains("resolvedfeatures");
        let section_is_admissible = !section.contains("ghost")
            && !section.contains("deltas")
            && !section.contains("resolvedfeatures");
        let body_streams = block
            .ps_streams
            .iter()
            .filter_map(|payload| {
                let header = crate::parasolid::stream_header(payload)?;
                crate::parasolid::is_body_stream(&header).then_some((payload, header))
            })
            .collect::<Vec<_>>();
        let sole_body_stream = body_streams.len() == 1;
        for (payload, header) in body_streams {
            let description = header.description.to_ascii_lowercase();
            if !section_is_admissible
                || description.contains("ghost")
                || description.contains("deltas")
                || !(description.contains("partition") || sole_body_stream && section_is_partition)
                || active_configuration.is_some_and(|active| {
                    block.section.as_deref().and_then(configuration_index) != Some(active)
                })
            {
                continue;
            }
            candidates.push(ActiveParasolidSite {
                origin: ActiveParasolidOrigin::Block(block),
                payload,
                header,
            });
        }
    }
    for stream in &scan.compound_streams {
        let path = stream.path.to_ascii_lowercase();
        let section_is_partition = path.contains("partition")
            && !path.contains("ghost")
            && !path.contains("deltas")
            && !path.contains("resolvedfeatures");
        let section_is_admissible = !path.contains("ghost")
            && !path.contains("deltas")
            && !path.contains("resolvedfeatures");
        let body_streams = stream
            .ps_streams
            .iter()
            .filter_map(|payload| {
                let header = crate::parasolid::stream_header(payload)?;
                crate::parasolid::is_body_stream(&header).then_some((payload, header))
            })
            .collect::<Vec<_>>();
        let sole_body_stream = body_streams.len() == 1;
        for (payload, header) in body_streams {
            let description = header.description.to_ascii_lowercase();
            if !section_is_admissible
                || description.contains("ghost")
                || description.contains("deltas")
                || !(description.contains("partition") || sole_body_stream && section_is_partition)
                || active_configuration
                    .is_some_and(|active| configuration_index(&stream.path) != Some(active))
            {
                continue;
            }
            candidates.push(ActiveParasolidSite {
                origin: ActiveParasolidOrigin::Compound(stream),
                payload,
                header,
            });
        }
    }
    (candidates.len() == 1).then(|| candidates.remove(0))
}

pub(crate) fn configuration_index(section: &str) -> Option<usize> {
    let start = section.to_ascii_lowercase().find("config-")? + "config-".len();
    let digits = section[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

pub(crate) fn active_configuration_index(scan: &ContainerScan) -> Option<usize> {
    explicit_active_configuration_index(scan)
        .or_else(|| manifest_active_configuration(scan).map(|(index, _)| index))
}

/// Return the active configuration's unique manifest identity, when one
/// manifest row provides it. A manifest with zero or several `YES` rows is
/// deliberately not an index source.
pub(crate) fn manifest_active_configuration(
    scan: &ContainerScan<'_>,
) -> Option<(usize, Option<String>)> {
    let mut candidate = None;
    for section in scan.sections() {
        if !is_features_manifest(section) {
            continue;
        }
        let Some(text) = xml_text(section.payload()) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        if document.root_element().tag_name().name() != "swSolidWorks" {
            continue;
        }
        let configurations = document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "swConfiguration")
            .collect::<Vec<_>>();
        if configurations.is_empty() {
            continue;
        }
        let rows = configurations
            .into_iter()
            .filter(|node| node.attribute("swMostRecentConfiguration") == Some("YES"))
            .collect::<Vec<_>>();
        let [row] = rows.as_slice() else {
            return None;
        };
        let id = row.attribute("swID")?;
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let index = id.parse::<usize>().ok()?;
        let name = manifest_configuration_name(&document, *row, id);
        let current = (index, name);
        match candidate.as_mut() {
            None => candidate = Some(current),
            Some((previous_index, previous_name)) => {
                if *previous_index != index {
                    return None;
                }
                if let (Some(previous), Some(current)) =
                    (previous_name.as_deref(), current.1.as_deref())
                {
                    if previous != current {
                        return None;
                    }
                }
                if previous_name.is_none() {
                    *previous_name = current.1;
                }
            }
        }
    }
    candidate
}

fn is_features_manifest(section: Section<'_>) -> bool {
    section
        .name()
        .and_then(|name| name.rsplit('/').next())
        .is_some_and(|name| name.eq_ignore_ascii_case("Features"))
}

/// Resolve a manifest row to its configuration name without depending on XML
/// namespace prefixes or on the order of the model rows.
fn manifest_configuration_name(
    document: &roxmltree::Document<'_>,
    row: roxmltree::Node<'_, '_>,
    id: &str,
) -> Option<String> {
    let direct = row
        .attribute("swName")
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let model = row
        .attribute("swModelRef")
        .and_then(|reference| {
            document.descendants().find(|node| {
                node.tag_name().name() == "swModel" && node.attribute("id") == Some(reference)
            })
        })
        .or_else(|| {
            document.descendants().find(|node| {
                node.tag_name().name() == "swModel"
                    && node.attribute("swConfigurationId") == Some(id)
            })
        });
    direct.or_else(|| {
        model
            .and_then(|node| node.attribute("swConfigurationName"))
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn explicit_active_configuration_index(scan: &ContainerScan<'_>) -> Option<usize> {
    let active = active_configuration_name(scan)?;
    let mut indices = Vec::new();
    for section in scan.sections() {
        let Some(text) = xml_text(section.payload()) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        if !document
            .root_element()
            .tag_name()
            .name()
            .contains("Keywords")
        {
            continue;
        }
        indices.extend(
            document
                .descendants()
                .filter(|node| {
                    node.is_element()
                        && node.tag_name().name() == "Configuration"
                        && node.attribute("Name") == Some(active.as_str())
                })
                .filter_map(|node| node.attribute("SourceIndex"))
                .filter(|value| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                })
                .filter_map(|value| value.parse::<usize>().ok()),
        );
    }
    (indices.len() == 1).then(|| indices[0])
}

pub(crate) fn active_configuration_name(scan: &ContainerScan<'_>) -> Option<String> {
    manifest_active_configuration(scan)
        .and_then(|(_, name)| name)
        .or_else(|| {
            let mut names = BTreeSet::new();
            for section in scan.sections() {
                let Some(text) = xml_text(section.payload()) else {
                    continue;
                };
                let Ok(document) = roxmltree::Document::parse(&text) else {
                    continue;
                };
                if document.root_element().tag_name().name() != "swSolidWorks" {
                    continue;
                }
                names.extend(
                    document
                        .descendants()
                        .filter(|node| node.tag_name().name() == "swModel")
                        .filter_map(|node| node.attribute("swConfigurationName"))
                        .map(str::to_string),
                );
            }
            (names.len() == 1)
                .then(|| names.into_iter().next())
                .flatten()
        })
}

pub(crate) fn xml_text(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) {
        let mut view = View::over_retained(&bytes[2..]);
        let mut units = Vec::new();
        while let Some(unit) = view.u16_le() {
            units.push(unit);
        }
        Some(String::from_utf16_lossy(&units))
    } else {
        std::str::from_utf8(bytes).ok().map(str::to_string)
    }
}

/// Metadata from the first parsed `swSolidWorks` envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SolidWorksEnvelope {
    pub(crate) sw_version: Option<String>,
    pub(crate) creation_time: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) model_name: Option<String>,
    pub(crate) configuration_name: Option<String>,
    pub(crate) configuration_attributes: BTreeMap<String, String>,
}

/// Parses the first `swSolidWorks` envelope and stops even if an attribute is absent.
pub(crate) fn first_solidworks_envelope<'a>(
    payloads: impl IntoIterator<Item = &'a [u8]>,
) -> Option<SolidWorksEnvelope> {
    for payload in payloads {
        if payload_family(payload) != "xml" {
            continue;
        }
        let Some(text) = xml_text(payload) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let root = document.root_element();
        if root.tag_name().name() != "swSolidWorks" {
            continue;
        }
        let model = root.descendants().find(|node| node.has_tag_name("swModel"));
        let mut configuration_attributes = BTreeMap::new();
        for configuration in root
            .descendants()
            .filter(|node| node.has_tag_name("swConfiguration"))
        {
            let Some(slot) = configuration.attribute("swID") else {
                continue;
            };
            if !slot.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            for (source, target) in [
                ("swConfigurationNeedsUpdate", "needs_update"),
                ("swMostRecentConfiguration", "most_recent"),
                ("swConfigurationFlags", "flags"),
                ("swConfigurationAlternateName", "alternate_name"),
            ] {
                if let Some(value) = configuration.attribute(source) {
                    configuration_attributes.insert(
                        format!("sw_configuration_{slot}_{target}"),
                        value.to_owned(),
                    );
                }
            }
        }
        return Some(SolidWorksEnvelope {
            sw_version: root.attribute("swVersion").map(str::to_owned),
            creation_time: root.attribute("swCreationTime").map(str::to_owned),
            path: root.attribute("swPath").map(str::to_owned),
            model_name: model
                .and_then(|node| node.attribute("swName"))
                .map(str::to_owned),
            configuration_name: model
                .and_then(|node| node.attribute("swConfigurationName"))
                .map(str::to_owned),
            configuration_attributes,
        });
    }
    None
}

pub(crate) fn solidworks_envelope(scan: &ContainerScan<'_>) -> Option<SolidWorksEnvelope> {
    first_solidworks_envelope(scan.sections().map(Section::payload))
}

/// Returns the first envelope's `swVersion` declaration verbatim.
pub(crate) fn declared_sw_version(scan: &ContainerScan<'_>) -> Option<String> {
    solidworks_envelope(scan).and_then(|envelope| envelope.sw_version)
}

#[cfg(test)]
mod tests;
