// SPDX-License-Identifier: Apache-2.0
//! Outer `.sldprt` container scanning and inspection.
//!
//! Files start with an 8-byte `file_id` and big-endian version header. A shared
//! marker introduces raw-DEFLATE blocks, cache cells, and tail-directory
//! entries. [`scan`] classifies marker occurrences with structure-specific
//! invariants, validates block CRC-32 values, inflates payloads, decodes stored
//! section names, and extracts embedded Parasolid streams.

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::compound::{CompoundEntry, CompoundPrefixProbe, CompoundSnapshot};
use cadmpeg_container::compression::{
    inflate_bounded_probe, inflate_deflate_owned, inflate_zlib_member_owned,
};
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ScopedReservation, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ContainerSummary;

use crate::layout::block_frame_header as block_hdr;
use crate::layout::cache_cell_header as cache_hdr;
use crate::layout::outer_header as outer_hdr;
use crate::layout::tail_directory_entry as dir_ent;
use crate::layout::zlb_wrapper_header as zlb_hdr;

/// Marker shared by block, cache-cell, and directory frames.
pub(crate) const MARKER: [u8; 6] = block_hdr::MARKER_VALUE;

/// Upper bound on a single decompressed block, guarding a corrupt `uncomp_sz`
/// from driving an unbounded allocation. Real part streams sit far below this.
const MAX_UNCOMP: usize = 512 * 1024 * 1024;

/// Classified decompressed payload signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadFamily {
    Parasolid,
    PngPreview,
    BmpThumbnail,
    Ole2,
    Tessellation,
    SwObjects,
    Unqlite,
    Xml,
    Unknown,
}

impl PayloadFamily {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Parasolid => "parasolid",
            Self::PngPreview => "png-preview",
            Self::BmpThumbnail => "bmp-thumbnail",
            Self::Ole2 => "ole2",
            Self::Tessellation => "tessellation",
            Self::SwObjects => "sw-objects",
            Self::Unqlite => "unqlite",
            Self::Xml => "xml",
            Self::Unknown => "unknown",
        }
    }
}

/// Classify a decompressed block payload by signature.
///
/// Unknown signatures return [`PayloadFamily::Unknown`].
pub(crate) fn payload_family(payload: &[u8]) -> PayloadFamily {
    if payload.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        PayloadFamily::PngPreview
    } else if is_bmp_thumbnail(payload) {
        PayloadFamily::BmpThumbnail
    } else if payload.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        PayloadFamily::Ole2
    } else if contains(payload, b"uoTempBodyTessData_c")
        || contains(payload, b"uoTempFaceTessData_c")
    {
        PayloadFamily::Tessellation
    } else if payload.starts_with(&[0xff, 0xff, 0x01, 0x00]) {
        PayloadFamily::SwObjects
    } else if payload.starts_with(b"unqlite") {
        PayloadFamily::Unqlite
    } else if payload.starts_with(b"<?xml")
        || payload.starts_with(&[0xff, 0xfe])
        || (payload.first() == Some(&0x86) && contains(&payload[..payload.len().min(64)], b"<"))
    {
        PayloadFamily::Xml
    } else {
        PayloadFamily::Unknown
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

/// Decode a nibble-swapped section name.
///
/// Returns `None` when any decoded byte falls outside printable ASCII.
fn nibble_swap_name(raw: &[u8]) -> Option<String> {
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

fn nibble_swap_name_charged(
    ctx: &DecodeContext<'_>,
    raw: &[u8],
) -> Result<Option<String>, CodecError> {
    if !raw
        .iter()
        .all(|byte| (0x20..0x7f).contains(&byte.rotate_left(4)))
    {
        return Ok(None);
    }
    let mut bytes = ctx.copy_retained(raw, "retain SLDPRT section name")?;
    for byte in &mut bytes {
        *byte = byte.rotate_left(4);
    }
    Ok(String::from_utf8(bytes).ok())
}

/// The admitted name of one compressed block.
///
/// Anonymous blocks retain a generated source owner while keeping their
/// section classification absent. Named and anonymous blocks therefore share
/// one owning value instead of carrying two writable copies of the name.
#[derive(Debug, Clone)]
pub(crate) enum BlockName {
    Named(cadmpeg_ir::StreamName),
    Anonymous(cadmpeg_ir::StreamName),
}

impl BlockName {
    pub(crate) fn name(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name.as_str()),
            Self::Anonymous(_) => None,
        }
    }

    pub(crate) fn source_stream(&self) -> &cadmpeg_ir::StreamName {
        match self {
            Self::Named(name) | Self::Anonymous(name) => name,
        }
    }
}

/// One validated compressed block.
#[derive(Debug, Clone)]
pub(crate) struct Block {
    /// Byte offset of the marker in the file.
    pub(crate) offset: usize,
    /// Frame `type_id`.
    pub(crate) type_id: u32,
    /// Compressed payload length.
    pub(crate) comp_sz: u32,
    /// OPC section name and admitted source owner.
    pub(crate) section: BlockName,
    /// Payload family from its signature or extracted Parasolid streams.
    pub(crate) family: PayloadFamily,
    /// The decompressed payload bytes.
    pub(crate) payload: Vec<u8>,
    /// Every located and header-validated Parasolid stream carried by this block.
    pub(crate) ps_streams: Vec<crate::parasolid::ExtractedStream>,
}

impl Block {
    /// Decompressed payload length.
    pub(crate) fn uncomp_sz(&self) -> usize {
        self.payload.len()
    }
}

/// One tail-directory entry naming a section.
#[derive(Debug, Clone)]
pub(crate) struct DirectoryEntry {
    /// Byte offset of the marker.
    pub(crate) offset: usize,
    /// Frame `type_id`.
    pub(crate) type_id: u32,
    /// The section's stored/uncompressed size.
    pub(crate) size: u32,
    /// Decoded section name.
    pub(crate) name: String,
    /// Per-entry descriptor bytes at frame offset +26.
    pub(crate) descriptor: [u8; 14],
    /// File-level directory trailer following the encoded name.
    pub(crate) trailer: [u8; 6],
}

/// One cache-cell section-index entry.
#[derive(Debug, Clone)]
pub(crate) struct CacheCell {
    /// Byte offset of the marker.
    pub(crate) offset: usize,
    /// The logical cell size `L`.
    pub(crate) logical_len: u32,
    /// Decoded section name.
    pub(crate) name: String,
}

/// One named stream in a Compound File Binary container.
#[derive(Debug, Clone)]
pub(crate) struct CompoundStream {
    /// Storage-qualified stream path and admitted source owner.
    pub(crate) path: cadmpeg_ir::StreamName,
    /// Unique directory entry identifier.
    pub(crate) directory_id: u32,
    /// First regular or mini sector identifier.
    pub(crate) start_sector: u32,
    /// Exact stream bytes.
    pub(crate) payload: Vec<u8>,
    /// Inflated semantic bytes when the stream uses the `__ZLB` wrapper.
    pub(crate) decoded_payload: Option<Vec<u8>>,
    /// Every located and header-validated Parasolid stream carried here.
    pub(crate) ps_streams: Vec<crate::parasolid::ExtractedStream>,
}

/// Complete result of an outer-container scan.
pub(crate) struct ContainerScan<'a> {
    /// Complete source image for exact passthrough writing.
    pub(crate) source_image: &'a [u8],
    /// Big-endian outer version word.
    pub(crate) version: u32,
    /// CRC-validated compressed blocks, in file order.
    pub(crate) blocks: Vec<Block>,
    /// Tail directory entries, in file order.
    pub(crate) directory: Vec<DirectoryEntry>,
    /// Cache-cell grid entries, in file order.
    pub(crate) cache_cells: Vec<CacheCell>,
    /// Named streams when the source uses the Compound File Binary envelope.
    pub(crate) compound_streams: Vec<CompoundStream>,
    /// `swSolidWorks` XML facts parsed once from the retained sections.
    pub(crate) solidworks: SolidWorksEnvelopeScan,
}

#[derive(Clone, Copy)]
pub(crate) enum Section<'a> {
    Block(&'a Block),
    Compound(&'a CompoundStream),
}

impl<'a> Section<'a> {
    pub(crate) fn name(self) -> Option<&'a str> {
        match self {
            Self::Block(block) => block.section.name(),
            Self::Compound(stream) => Some(stream.path.as_str()),
        }
    }

    fn display_name(self) -> String {
        self.source_stream().as_str().to_owned()
    }

    pub(crate) fn source_stream(self) -> &'a cadmpeg_ir::StreamName {
        match self {
            Self::Block(block) => block.section.source_stream(),
            Self::Compound(stream) => &stream.path,
        }
    }

    pub(crate) fn ordinal(self) -> usize {
        match self {
            Self::Block(block) => block.offset,
            Self::Compound(stream) => stream.directory_id as usize,
        }
    }

    pub(crate) fn native_id(self) -> cadmpeg_ir::ids::UnknownId {
        match self {
            Self::Block(block) => cadmpeg_ir::ids::UnknownId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "file", "block"),
                block.offset,
            ),
            Self::Compound(stream) => cadmpeg_ir::ids::UnknownId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "file", "compound-stream"),
                stream.directory_id,
            ),
        }
    }

    pub(crate) fn site_key(self) -> String {
        match self {
            Self::Block(block) => format!("block@{}", block.offset),
            Self::Compound(stream) => format!("compound@{}", stream.directory_id),
        }
    }

    pub(crate) fn payload(self) -> &'a [u8] {
        match self {
            Self::Block(block) => &block.payload,
            Self::Compound(stream) => stream.decoded_payload.as_deref().unwrap_or(&stream.payload),
        }
    }

    pub(crate) fn ps_streams(self) -> &'a [crate::parasolid::ExtractedStream] {
        match self {
            Self::Block(block) => &block.ps_streams,
            Self::Compound(stream) => &stream.ps_streams,
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

pub(crate) fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Test whether a prefix contains the container marker after its outer header.
///
/// This structural check does not validate block framing or CRC-32.
pub(crate) fn looks_like_sldprt(prefix: &[u8]) -> bool {
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

/// Scan an in-memory `.sldprt` image.
///
/// Truncated input produces a scan containing every structure that could be
/// validated; missing outer-header bytes yield version zero.
pub(crate) fn scan_bytes(bytes: &[u8]) -> ContainerScan<'_> {
    if bytes.starts_with(&COMPOUND_FILE_MAGIC) {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let compound_streams = DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .ok()
            .and_then(|(ctx, root)| compound_streams(&ctx, root).ok())
            .unwrap_or_default();
        return completed_scan(
            bytes,
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            compound_streams,
        );
    }
    let version = native_version(bytes);
    let (blocks, directory, cache_cells) = match walk_native_markers(
        bytes,
        ScanAdmission::Probe,
        |off| Ok(try_block(bytes, off)),
        |off| Ok(try_cache_cell(bytes, off)),
        |off| Ok(try_directory_entry(bytes, off)),
    ) {
        Ok(frames) => frames,
        Err(_) => (Vec::new(), Vec::new(), Vec::new()),
    };

    completed_scan(bytes, version, blocks, directory, cache_cells, Vec::new())
}

fn completed_scan(
    source_image: &[u8],
    version: u32,
    blocks: Vec<Block>,
    directory: Vec<DirectoryEntry>,
    cache_cells: Vec<CacheCell>,
    compound_streams: Vec<CompoundStream>,
) -> ContainerScan<'_> {
    let mut scan = assemble_scan(source_image, version, blocks, directory, cache_cells, compound_streams);
    let solidworks = scan_solidworks_envelopes(
        scan.sections().map(|section| (section.name(), section.payload())),
        ScanAdmission::Probe,
    )
    .unwrap_or_default();
    scan.solidworks = solidworks;
    scan
}

fn completed_scan_charged<'a>(
    ctx: &DecodeContext<'_>,
    source_image: &'a [u8],
    version: u32,
    blocks: Vec<Block>,
    directory: Vec<DirectoryEntry>,
    cache_cells: Vec<CacheCell>,
    compound_streams: Vec<CompoundStream>,
) -> Result<ContainerScan<'a>, CodecError> {
    let mut scan = assemble_scan(source_image, version, blocks, directory, cache_cells, compound_streams);
    let solidworks = scan_solidworks_envelopes(
        scan.sections().map(|section| (section.name(), section.payload())),
        ScanAdmission::Decode(ctx),
    )?;
    scan.solidworks = solidworks;
    Ok(scan)
}

fn assemble_scan(
    source_image: &[u8],
    version: u32,
    blocks: Vec<Block>,
    directory: Vec<DirectoryEntry>,
    cache_cells: Vec<CacheCell>,
    compound_streams: Vec<CompoundStream>,
) -> ContainerScan<'_> {
    ContainerScan {
        source_image,
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams,
        solidworks: SolidWorksEnvelopeScan::default(),
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
type NativeWalk = Result<(Vec<Block>, Vec<DirectoryEntry>, Vec<CacheCell>), CodecError>;

enum ScanAdmission<'a, 'ctx> {
    Probe,
    Decode(&'ctx DecodeContext<'a>),
}

impl ScanAdmission<'_, '_> {
    fn reserve<T>(&self, values: &mut Vec<T>, operation: &'static str) -> Result<(), CodecError> {
        if let Self::Decode(ctx) = self {
            ctx.reserve_collection_vec(values, 1, operation)?;
            ctx.charge_entities(1, operation)?;
        }
        Ok(())
    }
}

fn walk_native_markers(
    bytes: &[u8],
    admission: ScanAdmission<'_, '_>,
    mut try_one_block: impl FnMut(usize) -> Result<Option<RawBlock>, CodecError>,
    mut try_one_cell: impl FnMut(usize) -> Result<Option<CacheCell>, CodecError>,
    mut try_one_directory: impl FnMut(usize) -> Result<Option<DirectoryEntry>, CodecError>,
) -> NativeWalk {
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
            admission.reserve(&mut blocks, "admit SLDPRT block")?;
            blocks.push(block.into_block());
            continue;
        }
        if let Some(cell) = try_one_cell(i)? {
            admission.reserve(&mut cache_cells, "admit SLDPRT cache cell")?;
            cache_cells.push(cell);
        } else if let Some(entry) = try_one_directory(i)? {
            admission.reserve(&mut directory, "admit SLDPRT directory entry")?;
            directory.push(entry);
        }
        i += 1;
    }
    Ok((blocks, directory, cache_cells))
}

fn compound_stream(
    ctx: &DecodeContext<'_>,
    path: String,
    directory_id: u32,
    start_sector: u32,
    bytes: Vec<u8>,
    decoded_bytes: Option<Vec<u8>>,
) -> Result<CompoundStream, CodecError> {
    let ps_streams = crate::parasolid::extract_streams_with_offsets(&bytes, ctx)?;
    let path = match cadmpeg_ir::StreamName::try_from(path) {
        Ok(path) => path,
        Err(_) => cadmpeg_ir::stream_name!("compound@").with_suffix(directory_id),
    };
    Ok(CompoundStream {
        path,
        directory_id,
        start_sector,
        payload: bytes,
        decoded_payload: decoded_bytes,
        ps_streams,
    })
}

/// Scans an in-memory image while routing inflate through the decode budget.
pub(crate) fn scan<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    if root.window().starts_with(&COMPOUND_FILE_MAGIC) {
        let compound_streams = compound_streams(ctx, root)?;
        return completed_scan_charged(
            ctx,
            root.window(),
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            compound_streams,
        );
    }
    let bytes = root.window();
    let version = native_version(bytes);
    let (blocks, directory, cache_cells) = walk_native_markers(
        bytes,
        ScanAdmission::Decode(ctx),
        |off| try_block_budgeted(ctx, root, off),
        |off| try_cache_cell_with(bytes, off, |raw| nibble_swap_name_charged(ctx, raw)),
        |off| try_directory_entry_with(bytes, off, |raw| nibble_swap_name_charged(ctx, raw)),
    )?;
    completed_scan_charged(
        ctx,
        bytes,
        version,
        blocks,
        directory,
        cache_cells,
        Vec::new(),
    )
}

/// CFB directory/FAT/open is [`CompoundSnapshot`]; ZLB unwrap and Parasolid
/// extract stay codec-local because they are `SolidWorks` payload semantics, not
/// CFB.
fn compound_streams<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<Vec<CompoundStream>, CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    let mut streams = Vec::new();
    for entry in snapshot.entries() {
        let CompoundEntry::Stream(entry) = entry else {
            continue;
        };
        let view = snapshot.open(ctx, entry)?;
        let payload = ctx.copy_retained(view.window(), "retain SolidWorks CFB stream")?;
        let decoded = decode_wrapped_payload_budgeted(ctx, view)?;
        let mut path = String::new();
        ctx.reserve_retained_string(&mut path, entry.path().len(), "retain SLDPRT stream path")?;
        path.push_str(entry.path());
        let stream = compound_stream(
            ctx,
            path,
            entry.id().directory_id(),
            entry.start_sector(),
            payload,
            decoded,
        )?;
        ctx.reserve_collection_vec(&mut streams, 1, "admit SLDPRT compound stream")?;
        streams.push(stream);
    }
    Ok(streams)
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
        match inflate_zlib_member_owned(ctx, member, ExpandSpec::Exact(uncompressed_size)) {
            Ok(result) => result,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(None),
        };
    if consumed != compressed_size {
        return Ok(None);
    }
    Ok(Some(decoded))
}

/// A block plus the preamble length needed to advance past it.
struct RawBlock {
    offset: usize,
    type_id: u32,
    comp_sz: u32,
    preamble_len: usize,
    section: Option<String>,
    family: PayloadFamily,
    payload: Vec<u8>,
    ps_streams: Vec<crate::parasolid::ExtractedStream>,
}

impl RawBlock {
    fn into_block(self) -> Block {
        let section = match self.section {
            Some(name) => match cadmpeg_ir::StreamName::try_from(name) {
                Ok(name) => BlockName::Named(name),
                Err(_) => BlockName::Anonymous(
                    cadmpeg_ir::stream_name!("block@").with_suffix(self.offset),
                ),
            },
            None => {
                BlockName::Anonymous(cadmpeg_ir::stream_name!("block@").with_suffix(self.offset))
            }
        };
        Block {
            offset: self.offset,
            type_id: self.type_id,
            comp_sz: self.comp_sz,
            section,
            family: self.family,
            payload: self.payload,
            ps_streams: self.ps_streams,
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
    // discarded-value: the payload range is proven to lie in the block; ? states the refusal and the slice has no reader
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    off: usize,
    frame: &BlockFrame,
    inflated: Vec<u8>,
) -> Result<Option<RawBlock>, CodecError> {
    if inflated.len() != frame.uncomp_sz as usize {
        return Ok(None);
    }
    if crc32fast::hash(&inflated) != frame.crc {
        return Ok(None);
    }

    let payload_start = off + block_hdr::LEN + frame.pre_sz as usize;
    let preamble = bytes
        .get(off + block_hdr::LEN..payload_start)
        .unwrap_or(&[]);
    let section = nibble_swap_name_charged(ctx, preamble)?;
    // A Parasolid block is one from which a `PS\0\0` stream can be extracted (in
    // plain, wrapped, or nested form); otherwise fall back to a byte-signature
    // family label.
    let ps_streams = crate::parasolid::extract_streams_with_offsets(&inflated, ctx)?;
    let family = if ps_streams.is_empty() {
        payload_family(&inflated)
    } else {
        PayloadFamily::Parasolid
    };

    Ok(Some(RawBlock {
        offset: off,
        type_id: frame.type_id,
        comp_sz: frame.comp_sz,
        preamble_len: frame.pre_sz as usize,
        section,
        family,
        payload: inflated,
        ps_streams,
    }))
}

fn try_block(bytes: &[u8], off: usize) -> Option<RawBlock> {
    let (frame, payload_start, payload_end) = read_block_frame(bytes, off)?;
    let payload = bytes.get(payload_start..payload_end)?;
    let inflated = inflate_bounded_probe(payload, frame.uncomp_sz as usize)?;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).ok()?;
    block_from_inflated(&ctx, bytes, off, &frame, inflated)
        .ok()
        .flatten()
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
    let inflated = match inflate_deflate_owned(
        ctx,
        payload_view,
        ExpandSpec::Exact(u64::from(frame.uncomp_sz)),
    ) {
        Ok(view) => view,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    block_from_inflated(ctx, bytes, off, &frame, inflated)
}

/// Test a marker hit against the cache-cell relational invariant
/// (`f@+10 == 2L`, `f@+14 == L/2`, `f@+18 == L`, `f@+22 == name_len`) plus a
/// printable nibble-swapped name ([spec §2.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#12-cache-cell-section-index-grid)).
fn try_cache_cell(bytes: &[u8], off: usize) -> Option<CacheCell> {
    match try_cache_cell_with(bytes, off, |raw| {
        Ok::<_, std::convert::Infallible>(nibble_swap_name(raw))
    }) {
        Ok(cell) => cell,
        Err(never) => match never {},
    }
}

fn try_cache_cell_with<E>(
    bytes: &[u8],
    off: usize,
    name_from_bytes: impl FnOnce(&[u8]) -> Result<Option<String>, E>,
) -> Result<Option<CacheCell>, E> {
    let Some((two_l, half_l, l, name_len)) = (|| {
        Some((
            View::u32_le_at(bytes, off + cache_hdr::TWO_L)?,
            View::u32_le_at(bytes, off + cache_hdr::HALF_L)?,
            View::u32_le_at(bytes, off + cache_hdr::L)?,
            View::u32_le_at(bytes, off + cache_hdr::NAME_LEN)?,
        ))
    })() else {
        return Ok(None);
    };

    if l == 0 || l.checked_mul(2) != Some(two_l) || half_l != l / 2 {
        return Ok(None);
    }
    if name_len == 0 || name_len >= 500 {
        return Ok(None);
    }
    let name_start = off + cache_hdr::LEN;
    let Some(raw) = bytes.get(name_start..name_start + name_len as usize) else {
        return Ok(None);
    };
    let Some(name) = name_from_bytes(raw)? else {
        return Ok(None);
    };
    Ok(Some(CacheCell {
        offset: off,
        logical_len: l,
        name,
    }))
}

/// Test a marker hit against the tail-directory frame: two zero words at +10 and
/// +18, a size at +14, a name length at +22, a 14-byte descriptor, then a
/// printable nibble-swapped name ([spec §2.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#13-tail-section-directory)).
fn try_directory_entry(bytes: &[u8], off: usize) -> Option<DirectoryEntry> {
    match try_directory_entry_with(bytes, off, |raw| {
        Ok::<_, std::convert::Infallible>(nibble_swap_name(raw))
    }) {
        Ok(entry) => entry,
        Err(never) => match never {},
    }
}

fn try_directory_entry_with<E>(
    bytes: &[u8],
    off: usize,
    name_from_bytes: impl FnOnce(&[u8]) -> Result<Option<String>, E>,
) -> Result<Option<DirectoryEntry>, E> {
    let Some((type_id, zero_a, size, zero_b, name_len)) = (|| {
        Some((
            View::u32_le_at(bytes, off + dir_ent::TYPE_ID)?,
            View::u32_le_at(bytes, off + dir_ent::ZERO_AT_10)?,
            View::u32_le_at(bytes, off + dir_ent::SIZE)?,
            View::u32_le_at(bytes, off + dir_ent::ZERO_AT_18)?,
            View::u32_le_at(bytes, off + dir_ent::NAME_LEN)?,
        ))
    })() else {
        return Ok(None);
    };
    if zero_a != 0 || zero_b != 0 {
        return Ok(None);
    }
    if name_len == 0 || name_len >= 500 {
        return Ok(None);
    }
    let name_start = off + dir_ent::LEN;
    let Some(raw) = bytes.get(name_start..name_start + name_len as usize) else {
        return Ok(None);
    };
    let Some(descriptor) = bytes
        .get(off + dir_ent::DESCRIPTOR..off + dir_ent::LEN)
        .and_then(|value| value.try_into().ok())
    else {
        return Ok(None);
    };
    let Some(trailer) = bytes
        .get(name_start + name_len as usize..name_start + name_len as usize + 6)
        .and_then(|value| value.try_into().ok())
    else {
        return Ok(None);
    };
    let Some(name) = name_from_bytes(raw)? else {
        return Ok(None);
    };
    Ok(Some(DirectoryEntry {
        offset: off,
        type_id,
        size,
        name,
        descriptor,
        trailer,
    }))
}

/// Convert a scan into the generic container inventory returned by
/// [`cadmpeg_ir::Codec::inspect`].
pub(crate) fn summarize(scan: &ContainerScan, dialects: DialectLayers) -> ContainerSummary {
    let mut entries = Vec::new();

    for b in &scan.blocks {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), b.offset.to_string());
        attributes.insert("type_id".to_string(), format!("0x{:08x}", b.type_id));
        attributes.insert("family".to_string(), b.family.label().to_string());
        attributes.insert("sha256".to_string(), sha256_hex(&b.payload));
        if let Some(stream) = b.ps_streams.first() {
            attributes.insert(
                "parasolid_schema".to_string(),
                stream.header.schema.value().to_owned(),
            );
            attributes.insert(
                "parasolid_description".to_string(),
                stream.header.description.clone(),
            );
        }
        entries.push(ContainerEntry {
            name: b.section.source_stream().as_str().to_owned(),
            role: ContainerRole::Block,
            storage: EntryStorage::Compressed {
                method: CompressionMethod::Deflate,
                stored: Some(b.comp_sz as u64),
                expanded: Some(b.uncomp_sz() as u64),
            },
            attributes,
        });
    }

    for d in &scan.directory {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), d.offset.to_string());
        attributes.insert("type_id".to_string(), format!("0x{:08x}", d.type_id));
        entries.push(ContainerEntry {
            name: d.name.clone(),
            role: ContainerRole::DirectoryEntry,
            storage: EntryStorage::payload_only(VerbatimLabel::None, d.size as u64),
            attributes,
        });
    }

    for c in &scan.cache_cells {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), c.offset.to_string());
        attributes.insert("logical_len".to_string(), c.logical_len.to_string());
        entries.push(ContainerEntry {
            name: c.name.clone(),
            role: ContainerRole::CacheCell,
            storage: EntryStorage::payload_only(VerbatimLabel::None, u64::from(c.logical_len)),
            attributes,
        });
    }

    for stream in &scan.compound_streams {
        let mut attributes = BTreeMap::new();
        attributes.insert("start_sector".to_string(), stream.start_sector.to_string());
        attributes.insert("sha256".to_string(), sha256_hex(&stream.payload));
        attributes.insert(
            "family".to_string(),
            payload_family(&stream.payload).label().to_string(),
        );
        entries.push(ContainerEntry {
            name: stream.path.as_str().to_owned(),
            role: ContainerRole::CompoundStream,
            storage: EntryStorage::verbatim(VerbatimLabel::Stored, stream.payload.len() as u64),
            attributes,
        });
    }

    ContainerSummary::classified(
        dialects,
        if scan.compound_streams.is_empty() {
            cadmpeg_ir::ContainerKind::SldprtBlocks
        } else {
            cadmpeg_ir::ContainerKind::CompoundFileBinary
        },
        entries,
        Vec::new(),
        notes(scan),
    )
}

/// Describe the decoded container without constructing its entry inventory.
pub(crate) fn notes(scan: &ContainerScan<'_>) -> Vec<String> {
    let active = match active_parasolid_summary(scan) {
        Some((name, size, sch)) => format!(
            "active Parasolid B-rep candidate: {} ({} bytes, schema {})",
            name, size, sch.schema
        ),
        None => NO_ACTIVE_PARASOLID_NOTE.to_string(),
    };
    notes_with_active(scan, active)
}

const NO_ACTIVE_PARASOLID_NOTE: &str =
    "no unique active Parasolid partition located; available B-rep sites remain decodable";

fn notes_with_active(scan: &ContainerScan<'_>, active: String) -> Vec<String> {
    vec![
        format!(
            "outer version word: 0x{:08x}; {} CRC-validated block(s), {} tail-directory \
         entry/entries, {} cache-cell(s), {} compound stream(s)",
            scan.version,
            scan.blocks.len(),
            scan.directory.len(),
            scan.cache_cells.len(),
            scan.compound_streams.len()
        ),
        active,
        "Parasolid body streams supply the typed topology and analytic carriers used by decode"
            .to_string(),
    ]
}

pub(crate) fn notes_charged(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Vec<String>, CodecError> {
    use std::fmt::Write;

    let active = match active_parasolid_summary(scan) {
        Some((name, size, sch)) => {
            const PREFIX: &str = "active Parasolid B-rep candidate: ";
            const MIDDLE: &str = " (";
            const SUFFIX: &str = " bytes, schema ";
            const CLOSE: &str = ")";
            let schema = sch.schema.value();
            let required = [
                PREFIX.len(),
                name.len(),
                MIDDLE.len(),
                size.to_string().len(),
                SUFFIX.len(),
                schema.len(),
                CLOSE.len(),
            ]
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("retain SLDPRT active site note", u64::MAX, u64::MAX)
            })?;
            let mut active = String::new();
            ctx.reserve_retained_string(
                &mut active,
                required,
                "retain SLDPRT active site note",
            )?;
            write!(active, "{PREFIX}{name}{MIDDLE}{size}{SUFFIX}{schema}{CLOSE}").map_err(
                |_| ctx.refuse_codec_limit("retain SLDPRT active site note", u64::MAX, u64::MAX),
            )?;
            active
        }
        None => NO_ACTIVE_PARASOLID_NOTE.to_string(),
    };
    Ok(notes_with_active(scan, active))
}

pub(crate) fn active_parasolid_summary<'a>(
    scan: &'a ContainerScan<'_>,
) -> Option<(&'a str, usize, &'a crate::parasolid::StreamHeader)> {
    let selected = select_active_parasolid_site(scan)?;
    Some((
        selected.section.source_stream().as_str(),
        selected.payload.len(),
        selected.header,
    ))
}

/// Test whether either outer envelope carries a framed Parasolid body stream.
pub(crate) fn has_parasolid_body_stream(scan: &ContainerScan) -> bool {
    scan.blocks
        .iter()
        .flat_map(|block| &block.ps_streams)
        .chain(
            scan.compound_streams
                .iter()
                .flat_map(|stream| &stream.ps_streams),
        )
        .any(|stream| crate::parasolid::is_body_stream(&stream.header))
}

/// One selected Parasolid partition stream and its source site.
pub(crate) struct ActiveParasolidSite<'a> {
    pub(crate) section: Section<'a>,
    pub(crate) payload: &'a [u8],
    pub(crate) header: &'a crate::parasolid::StreamHeader,
}

impl ActiveParasolidSite<'_> {
    pub(crate) fn name(&self) -> String {
        self.section.display_name()
    }

    pub(crate) fn source_stream(&self) -> &cadmpeg_ir::StreamName {
        self.section.source_stream()
    }

    pub(crate) fn site_key(&self) -> String {
        self.section.site_key()
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
    let mut selected = None;
    for section in scan.sections() {
        let name = section.name().unwrap_or("");
        let section_is_partition = contains_ascii_case_insensitive(name, "partition")
            && !contains_ascii_case_insensitive(name, "ghost")
            && !contains_ascii_case_insensitive(name, "deltas")
            && !contains_ascii_case_insensitive(name, "resolvedfeatures");
        let section_is_admissible = !contains_ascii_case_insensitive(name, "ghost")
            && !contains_ascii_case_insensitive(name, "deltas")
            && !contains_ascii_case_insensitive(name, "resolvedfeatures");
        let sole_body_stream = section
            .ps_streams()
            .iter()
            .filter(|stream| crate::parasolid::is_body_stream(&stream.header))
            .count()
            == 1;
        for stream in section.ps_streams() {
            if !crate::parasolid::is_body_stream(&stream.header) {
                continue;
            }
            let description = &stream.header.description;
            if !section_is_admissible
                || contains_ascii_case_insensitive(description, "ghost")
                || contains_ascii_case_insensitive(description, "deltas")
                || !(contains_ascii_case_insensitive(description, "partition")
                    || sole_body_stream && section_is_partition)
                || active_configuration.is_some_and(|active| {
                    section.name().and_then(configuration_index) != Some(active)
                })
            {
                continue;
            }
            if selected.is_some() {
                return None;
            }
            selected = Some(ActiveParasolidSite {
                section,
                payload: &stream.payload,
                header: &stream.header,
            });
        }
    }
    selected
}

pub(crate) fn configuration_index(section: &str) -> Option<usize> {
    let start = section
        .as_bytes()
        .windows(b"config-".len())
        .position(|window| window.eq_ignore_ascii_case(b"config-"))?
        + b"config-".len();
    let digit_count = section.as_bytes()[start..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digit_count > 0)
        .then(|| section[start..start + digit_count].parse().ok())
        .flatten()
}

pub(crate) fn active_configuration_index(scan: &ContainerScan) -> Option<usize> {
    explicit_active_configuration_index(scan)
        .or_else(|| {
            scan.solidworks
                .manifest_active_configuration
                .unique_ref()
                .map(|(index, _)| index)
        })
}

/// Return the active configuration's unique manifest identity, when one
/// manifest row provides it. A manifest with zero or several `YES` rows is
/// deliberately not an index source.
#[cfg(test)]
fn manifest_active_configuration(scan: &ContainerScan<'_>) -> Option<(usize, Option<String>)> {
    scan.solidworks.manifest_active_configuration.unique()
}

fn is_features_manifest_name(name: Option<&str>) -> bool {
    name.and_then(|name| name.rsplit('/').next())
        .is_some_and(|name| name.eq_ignore_ascii_case("Features"))
}

/// Resolve a manifest row to its configuration name without depending on XML
/// namespace prefixes or on the order of the model rows.
fn manifest_configuration_name(
    admission: &ScanAdmission<'_, '_>,
    document: &roxmltree::Document<'_>,
    row: roxmltree::Node<'_, '_>,
    id: &str,
) -> Result<Option<String>, CodecError> {
    let direct = row
        .attribute("swName")
        .filter(|value| !value.is_empty());
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
    let name = direct.or_else(|| {
        model
            .and_then(|node| node.attribute("swConfigurationName"))
            .filter(|value| !value.is_empty())
    });
    admission.copy_opt(name, "retain SLDPRT manifest configuration name")
}

fn explicit_active_configuration_index(scan: &ContainerScan<'_>) -> Option<usize> {
    let active = active_configuration_name_ref(scan)?;
    let indices = scan.solidworks.configuration_source_indices.get(active)?;
    (indices.len() == 1).then(|| indices[0])
}

pub(crate) fn active_configuration_name(scan: &ContainerScan<'_>) -> Option<String> {
    active_configuration_name_ref(scan).map(str::to_owned)
}

pub(crate) fn active_configuration_name_ref<'a>(scan: &'a ContainerScan<'_>) -> Option<&'a str> {
    scan.solidworks
        .manifest_active_configuration
        .unique_ref()
        .and_then(|(_, name)| name)
        .or_else(|| {
            (scan.solidworks.configuration_names.len() == 1)
                .then(|| scan.solidworks.configuration_names.iter().next().map(String::as_str))
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

pub(crate) struct EnvelopeText<'a> {
    text: String,
    _scope: Option<ScopedReservation<'a>>,
}

impl EnvelopeText<'_> {
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
}

impl ScanAdmission<'_, '_> {
    fn text<'scope>(&'scope self, bytes: &[u8]) -> Result<Option<EnvelopeText<'scope>>, CodecError> {
        match self {
            Self::Probe => Ok(xml_text(bytes).map(|text| EnvelopeText { text, _scope: None })),
            Self::Decode(ctx) => {
                xml_text_charged(ctx, bytes, "materialize SLDPRT XML text")
            }
        }
    }

    fn copy_str(&self, value: &str, operation: &'static str) -> Result<String, CodecError> {
        match self {
            Self::Probe => Ok(value.to_owned()),
            Self::Decode(ctx) => {
                let mut copy = String::new();
                ctx.reserve_retained_string(&mut copy, value.len(), operation)?;
                copy.push_str(value);
                Ok(copy)
            }
        }
    }

    fn copy_opt(&self, value: Option<&str>, operation: &'static str) -> Result<Option<String>, CodecError> {
        value.map(|value| self.copy_str(value, operation)).transpose()
    }

    fn charge_item(&self, operation: &'static str) -> Result<(), CodecError> {
        if let Self::Decode(ctx) = self {
            ctx.charge_collection_items(1, operation)?;
        }
        Ok(())
    }

    fn reserve_vec<T>(&self, values: &mut Vec<T>, operation: &'static str) -> Result<(), CodecError> {
        if let Self::Decode(ctx) = self {
            ctx.reserve_collection_vec(values, 1, operation)?;
        }
        Ok(())
    }

    fn new_string(&self, bytes: usize, operation: &'static str) -> Result<String, CodecError> {
        match self {
            Self::Probe => Ok(String::with_capacity(bytes)),
            Self::Decode(ctx) => {
                let mut value = String::new();
                ctx.reserve_retained_string(&mut value, bytes, operation)?;
                Ok(value)
            }
        }
    }
}

pub(crate) fn xml_text_charged<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Option<EnvelopeText<'ctx>>, CodecError> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) {
        let utf16 = &bytes[2..];
        let chars = || {
            char::decode_utf16(
                (0..utf16.len() / 2).filter_map(|index| View::u16_le_at(utf16, index * 2)),
            )
            .map(|result| match result {
                Ok(value) => value,
                Err(_) => char::REPLACEMENT_CHARACTER,
            })
        };
        let length = chars().try_fold(0usize, |length, value| {
            length.checked_add(value.len_utf8()).ok_or_else(|| {
                ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX)
            })
        })?;
        let (mut text, scope) = ctx.reserve_scoped_string(length, operation)?;
        for value in chars() {
            text.push(value);
        }
        Ok(Some(EnvelopeText { text, _scope: Some(scope) }))
    } else {
        let Ok(source) = std::str::from_utf8(bytes) else {
            return Ok(None);
        };
        let (mut text, scope) = ctx.reserve_scoped_string(source.len(), operation)?;
        text.push_str(source);
        Ok(Some(EnvelopeText { text, _scope: Some(scope) }))
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

/// Cached facts from all parsed `swSolidWorks` envelopes in one scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SolidWorksEnvelopeScan {
    first: Option<SolidWorksEnvelope>,
    configuration_names: BTreeSet<String>,
    manifest_active_configuration: ManifestActiveConfiguration,
    configuration_source_indices: BTreeMap<String, Vec<usize>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum ManifestActiveConfiguration {
    #[default]
    Absent,
    Unique(usize, Option<String>),
    Ambiguous,
}

impl ManifestActiveConfiguration {
    fn merge(&mut self, current: Self) {
        match (&mut *self, current) {
            (_, Self::Absent) | (Self::Ambiguous, _) => {}
            (_, Self::Ambiguous) => *self = Self::Ambiguous,
            (Self::Absent, current @ Self::Unique(..)) => *self = current,
            (Self::Unique(previous_index, previous_name), Self::Unique(index, mut name)) => {
                if *previous_index != index
                    || matches!(
                        (previous_name.as_deref(), name.as_deref()),
                        (Some(previous), Some(current)) if previous != current
                    )
                {
                    *self = Self::Ambiguous;
                } else if previous_name.is_none() {
                    *previous_name = name.take();
                }
            }
        }
    }

    #[cfg(test)]
    fn unique(&self) -> Option<(usize, Option<String>)> {
        self.unique_ref()
            .map(|(index, name)| (index, name.map(str::to_owned)))
    }

    fn unique_ref(&self) -> Option<(usize, Option<&str>)> {
        match self {
            Self::Unique(index, name) => Some((*index, name.as_deref())),
            Self::Absent | Self::Ambiguous => None,
        }
    }
}

fn scan_solidworks_envelopes<'a>(
    sections: impl IntoIterator<Item = (Option<&'a str>, &'a [u8])>,
    admission: ScanAdmission<'_, '_>,
) -> Result<SolidWorksEnvelopeScan, CodecError> {
    let mut scan = SolidWorksEnvelopeScan::default();
    for (section, payload) in sections {
        let Some(text) = admission.text(payload)? else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text.text) else {
            continue;
        };
        let root = document.root_element();
        if is_features_manifest_name(section) && root.tag_name().name() == "swSolidWorks" {
            scan.manifest_active_configuration
                .merge(manifest_active_configuration_in(&admission, &document)?);
        }
        if root.tag_name().name().contains("Keywords") {
            for configuration in document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
            {
                let Some(name) = configuration.attribute("Name") else {
                    continue;
                };
                let Some(index) = configuration
                    .attribute("SourceIndex")
                    .filter(|value| {
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                else {
                    continue;
                };
                if let Some(indices) = scan.configuration_source_indices.get_mut(name) {
                    admission.reserve_vec(indices, "collect SLDPRT configuration source indices")?;
                    indices.push(index);
                } else {
                    admission.charge_item("collect SLDPRT configuration source names")?;
                    let name = admission.copy_str(name, "retain SLDPRT configuration source name")?;
                    let mut indices = Vec::new();
                    admission.reserve_vec(&mut indices, "collect SLDPRT configuration source indices")?;
                    indices.push(index);
                    scan.configuration_source_indices.insert(name, indices);
                }
            }
        }
        if root.tag_name().name() != "swSolidWorks" {
            continue;
        }
        for name in root
            .descendants()
            .filter(|node| node.has_tag_name("swModel"))
            .filter_map(|node| node.attribute("swConfigurationName"))
        {
            if !scan.configuration_names.contains(name) {
                admission.charge_item("collect SLDPRT configuration names")?;
                let name = admission.copy_str(name, "retain SLDPRT configuration name")?;
                scan.configuration_names.insert(name);
            }
        }
        if scan.first.is_some() {
            continue;
        }
        let model = root.descendants().find(|node| node.has_tag_name("swModel"));
        let mut source_attributes = BTreeMap::new();
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
                    if !source_attributes.contains_key(&(slot, target)) {
                        admission.charge_item("collect SLDPRT configuration attributes")?;
                    }
                    source_attributes.insert((slot, target), value);
                }
            }
        }
        let mut configuration_attributes = BTreeMap::new();
        for ((slot, target), value) in source_attributes {
            let length = "sw_configuration_".len() + slot.len() + 1 + target.len();
            let mut key = admission.new_string(length, "retain SLDPRT configuration key")?;
            key.push_str("sw_configuration_");
            key.push_str(slot);
            key.push('_');
            key.push_str(target);
            let value = admission.copy_str(value, "retain SLDPRT configuration value")?;
            admission.charge_item("retain SLDPRT configuration attribute")?;
            configuration_attributes.insert(key, value);
        }
        scan.first = Some(SolidWorksEnvelope {
            sw_version: admission.copy_opt(root.attribute("swVersion"), "retain SLDPRT version")?,
            creation_time: admission.copy_opt(root.attribute("swCreationTime"), "retain SLDPRT creation time")?,
            path: admission.copy_opt(root.attribute("swPath"), "retain SLDPRT path")?,
            model_name: admission.copy_opt(model.and_then(|node| node.attribute("swName")), "retain SLDPRT model name")?,
            configuration_name: admission.copy_opt(model.and_then(|node| node.attribute("swConfigurationName")), "retain SLDPRT envelope configuration name")?,
            configuration_attributes,
        });
    }
    Ok(scan)
}

fn manifest_active_configuration_in(
    admission: &ScanAdmission<'_, '_>,
    document: &roxmltree::Document<'_>,
) -> Result<ManifestActiveConfiguration, CodecError> {
    let mut any_configuration = false;
    let mut active = None;
    for row in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "swConfiguration")
    {
        any_configuration = true;
        if row.attribute("swMostRecentConfiguration") == Some("YES") {
            if active.is_some() {
                return Ok(ManifestActiveConfiguration::Ambiguous);
            }
            active = Some(row);
        }
    }
    if !any_configuration {
        return Ok(ManifestActiveConfiguration::Absent);
    }
    let Some(row) = active else {
        return Ok(ManifestActiveConfiguration::Ambiguous);
    };
    let Some(id) = row.attribute("swID") else {
        return Ok(ManifestActiveConfiguration::Ambiguous);
    };
    let Some(index) = (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| id.parse::<usize>().ok())
        .flatten()
    else {
        return Ok(ManifestActiveConfiguration::Ambiguous);
    };
    Ok(ManifestActiveConfiguration::Unique(
        index,
        manifest_configuration_name(admission, document, row, id)?,
    ))
}

/// Returns the first parsed `swSolidWorks` envelope even if an attribute is absent.
pub(crate) fn first_solidworks_envelope<'a>(
    payloads: impl IntoIterator<Item = &'a [u8]>,
) -> Option<SolidWorksEnvelope> {
    scan_solidworks_envelopes(
        payloads.into_iter().map(|payload| (None, payload)),
        ScanAdmission::Probe,
    )
    .ok()
    .and_then(|scan| scan.first)
}

pub(crate) fn solidworks_envelope<'a>(
    scan: &'a ContainerScan<'_>,
) -> Option<&'a SolidWorksEnvelope> {
    scan.solidworks.first.as_ref()
}

/// Returns the first envelope's `swVersion` declaration verbatim.
pub(crate) fn declared_sw_version<'a>(scan: &'a ContainerScan<'_>) -> Option<&'a str> {
    solidworks_envelope(scan).and_then(|envelope| envelope.sw_version.as_deref())
}

#[cfg(test)]
mod tests;
