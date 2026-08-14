// SPDX-License-Identifier: Apache-2.0
//! Outer `.sldprt` container scanning and inspection.
//!
//! Files start with an 8-byte `file_id` and big-endian version header. A shared
//! marker introduces raw-DEFLATE blocks, cache cells, and tail-directory
//! entries. [`scan`] classifies marker occurrences with structure-specific
//! invariants, validates block CRC-32 values, inflates payloads, decodes stored
//! section names, and extracts embedded Parasolid streams.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use cadmpeg_container::compound::{CompoundEntry, CompoundPrefixProbe, CompoundSnapshot};
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
    payload[..window].windows(SIG.len()).position(|w| w == SIG)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
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
    let version = {
        let mut view = View::over_retained(bytes);
        view.seek(outer_hdr::VERSION)
            .and_then(|()| view.u32_be())
            .unwrap_or(0)
    };

    let mut blocks = Vec::new();
    let mut directory = Vec::new();
    let mut cache_cells = Vec::new();

    let mut i = outer_hdr::LEN;
    // Every marker hit is tried as a block first (the CRC gate is effectively
    // false-positive-free), then as a cache cell, then as a directory entry.
    while i + MARKER.len() <= bytes.len() {
        if bytes[i..i + MARKER.len()] != MARKER {
            i += 1;
            continue;
        }
        if let Some(block) = try_block(bytes, i) {
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

    ContainerScan {
        source_image: bytes,
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams: Vec::new(),
    }
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

/// Scans an in-memory image while routing CFB expansion through the decode budget.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<ContainerScan<'a>, CodecError> {
    if !root.window().starts_with(&COMPOUND_FILE_MAGIC) {
        return Ok(scan_bytes(root.window()));
    }
    let compound_streams = compound_streams(ctx, root)?;
    Ok(ContainerScan {
        source_image: root.window(),
        version: 0,
        blocks: Vec::new(),
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams,
    })
}

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
    let Some(member) = payload.get(zlb_hdr::LEN..member_end) else {
        return Ok(None);
    };
    let mut decoder = flate2::read::ZlibDecoder::new(member);
    let mut writer = ctx.begin_expand(source, ExpandSpec::Exact(uncompressed_size))?;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let Ok(read) = decoder.read(&mut chunk) else {
            return Ok(None);
        };
        if read == 0 {
            break;
        }
        if let Err(error) = writer.write(&chunk[..read]) {
            return match error {
                CodecError::ResourceLimit(_) => Err(error),
                _ => Ok(None),
            };
        }
    }
    if decoder.total_in() as usize != compressed_size {
        return Ok(None);
    }
    let decoded = match writer.finalize() {
        Ok(decoded) => decoded,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
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

fn try_block(bytes: &[u8], off: usize) -> Option<RawBlock> {
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
    let payload = bytes.get(payload_start..payload_start + comp)?;

    let inflated = raw_inflate(payload, uncomp)?;
    if inflated.len() != uncomp {
        return None;
    }
    if crc32(&inflated) != crc {
        return None;
    }

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
        type_id,
        comp_sz,
        uncomp_sz,
        preamble_len: pre,
        section,
        family,
        payload: inflated,
        ps_stream,
        ps_streams,
        ps_stream_offsets,
    })
}

/// Raw-DEFLATE (`wbits = -15`) inflate to at most `hint` bytes; `None` on any
/// decompression error (the CRC/round-trip gate rejects the marker hit).
fn raw_inflate(data: &[u8], hint: usize) -> Option<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    let mut out = Vec::new();
    out.try_reserve(hint.min(1 << 20)).ok()?;
    let mut dec = DeflateDecoder::new(data);
    let mut chunk = [0_u8; 8192];
    loop {
        let read = dec.read(&mut chunk).ok()?;
        if read == 0 {
            return Some(out);
        }
        if read > hint.saturating_sub(out.len()) {
            return None;
        }
        out.try_reserve(read).ok()?;
        out.extend_from_slice(&chunk[..read]);
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(bytes);
    h.finalize()
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

    ContainerSummary {
        format: "sldprt".to_string(),
        container_kind: if scan.compound_streams.is_empty() {
            "sldprt-blocks"
        } else {
            "compound-file-binary"
        }
        .to_string(),
        entries,
        notes,
    }
}

pub(crate) fn active_parasolid_summary(
    scan: &ContainerScan,
) -> Option<(String, usize, crate::parasolid::StreamHeader)> {
    if let Some((block, header)) = select_active_parasolid(scan) {
        return Some((
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            block.ps_stream.as_ref()?.len(),
            header,
        ));
    }
    let candidates = scan
        .compound_streams
        .iter()
        .flat_map(|stream| {
            stream.ps_streams.iter().filter_map(move |payload| {
                let header = crate::parasolid::stream_header(payload)?;
                let path = stream.path.to_ascii_lowercase();
                let description = header.description.to_ascii_lowercase();
                (crate::parasolid::is_body_stream(&header)
                    && !path.contains("ghost")
                    && !description.contains("ghost")
                    && (path.contains("partition") || description.contains("partition"))
                    && !path.contains("deltas")
                    && !description.contains("deltas"))
                .then_some((stream.path.clone(), payload.len(), header))
            })
        })
        .collect::<Vec<_>>();
    (candidates.len() == 1)
        .then(|| candidates.into_iter().next())
        .flatten()
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
/// block envelope must contain exactly one non-ghost partition candidate.
pub fn select_active_parasolid<'a>(
    scan: &'a ContainerScan<'_>,
) -> Option<(&'a Block, crate::parasolid::StreamHeader)> {
    let active_configuration = active_configuration_index(scan);
    let candidates = scan
        .blocks
        .iter()
        .flat_map(|block| {
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
                    crate::parasolid::is_body_stream(&header).then_some(header)
                })
                .collect::<Vec<_>>();
            let sole_body_stream = body_streams.len() == 1;
            body_streams
                .into_iter()
                .filter(move |header| {
                    let description = header.description.to_ascii_lowercase();
                    section_is_admissible
                        && !description.contains("ghost")
                        && !description.contains("deltas")
                        && (description.contains("partition")
                            || sole_body_stream && section_is_partition)
                })
                .map(move |header| (block, header))
                .collect::<Vec<_>>()
        })
        .filter(|(block, _)| {
            active_configuration.is_none_or(|active| {
                block.section.as_deref().and_then(configuration_index) == Some(active)
            })
        })
        .collect::<Vec<_>>();
    (candidates.len() == 1)
        .then(|| candidates.into_iter().next())
        .flatten()
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
    let active_names = scan
        .blocks
        .iter()
        .filter_map(|block| std::str::from_utf8(&block.payload).ok())
        .filter_map(|text| roxmltree::Document::parse(text).ok())
        .filter(|document| document.root_element().has_tag_name("swSolidWorks"))
        .flat_map(|document| {
            document
                .descendants()
                .filter(|node| node.has_tag_name("swModel"))
                .filter_map(|node| node.attribute("swConfigurationName").map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    let active = (active_names.len() == 1).then(|| active_names.first().cloned())??;
    let indices = scan
        .blocks
        .iter()
        .filter_map(|block| std::str::from_utf8(&block.payload).ok())
        .filter_map(|text| roxmltree::Document::parse(text).ok())
        .filter(|document| {
            document
                .root_element()
                .tag_name()
                .name()
                .contains("Keywords")
        })
        .flat_map(|document| {
            document
                .root_element()
                .children()
                .filter(|node| {
                    node.has_tag_name("Configuration")
                        && node.attribute("Name") == Some(active.as_str())
                })
                .map(|node| {
                    node.attribute("SourceIndex")
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    (indices.len() == 1).then(|| indices[0]).flatten()
}

#[cfg(test)]
mod tests;
