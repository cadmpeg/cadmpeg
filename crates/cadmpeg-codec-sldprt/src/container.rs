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
use cadmpeg_container::compression::{inflate_deflate_owned, inflate_zlib_member_owned};
use cadmpeg_core::decode::{
    index_from_u32, u64_from_index, DecodeContext, ExpandSpec, ScopedReservation, View,
};
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

/// Resource ceiling on the expansion of one native block.
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
pub(crate) fn payload_family(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    operation: &'static str,
) -> Result<PayloadFamily, CodecError> {
    ctx.charge_work(0, operation)?;
    Ok(if payload.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        PayloadFamily::PngPreview
    } else if is_bmp_thumbnail(payload) {
        PayloadFamily::BmpThumbnail
    } else if payload.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        PayloadFamily::Ole2
    } else if ctx.contains_bytes(payload, b"uoTempBodyTessData_c", operation)?
        || ctx.contains_bytes(payload, b"uoTempFaceTessData_c", operation)?
    {
        PayloadFamily::Tessellation
    } else if payload.starts_with(&[0xff, 0xff, 0x01, 0x00]) {
        PayloadFamily::SwObjects
    } else if payload.starts_with(b"unqlite") {
        PayloadFamily::Unqlite
    } else if payload.starts_with(b"<?xml")
        || payload.starts_with(&[0xff, 0xfe])
        || (payload.first() == Some(&0x86)
            && ctx.contains_bytes(&payload[..payload.len().min(64)], b"<", operation)?)
    {
        PayloadFamily::Xml
    } else {
        PayloadFamily::Unknown
    })
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

fn nibble_swap_name_charged(
    ctx: &DecodeContext<'_>,
    raw: &[u8],
) -> Result<Option<String>, CodecError> {
    let work = u64_from_index(raw.len()).checked_mul(3).ok_or_else(|| {
        ctx.refuse_codec_limit("validate SLDPRT section name", u64::MAX, u64::MAX)
    })?;
    ctx.charge_work(work, "validate SLDPRT section name")?;
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
            Self::Compound(stream) => index_from_u32(stream.directory_id),
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
    pub(crate) fn sections(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = Section<'_>>, CodecError> {
        let blocks = ctx.admit_iter(&self.blocks, "scan SLDPRT block sections")?;
        let streams = ctx.admit_iter(&self.compound_streams, "scan SLDPRT compound sections")?;
        Ok(blocks.map(Section::Block).chain(streams.map(Section::Compound)))
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
pub(crate) fn looks_like_sldprt(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    if prefix.starts_with(&COMPOUND_FILE_MAGIC) {
        let (probe, _storage) =
            CompoundPrefixProbe::inspect_with_context(ctx, View::over_retained(prefix))?;
        if let Some(paths) = probe.paths() {
            for path in paths {
                ctx.charge_work(
                    u64_from_index(path.len()),
                    "compare SolidWorks directory evidence",
                )?;
                ctx.charge_work(
                    u64_from_index(path.len()),
                    "scan SolidWorks directory basename",
                )?;
                if path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("ISolidWorksInformation"))
                {
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    }
    if prefix.len() < outer_hdr::LEN + MARKER.len() {
        return Ok(false);
    }
    Ok(ctx.admit_iter(&prefix[outer_hdr::LEN..], "scan SolidWorks detection marker")?
        .windows(std::num::NonZeroUsize::new(MARKER.len()).ok_or_else(|| CodecError::malformed("zero detection marker width"))?)
        .any(|w| w == MARKER))
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
    let mut scan = ContainerScan {
        source_image,
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams,
        solidworks: SolidWorksEnvelopeScan::default(),
    };
    let solidworks = scan_solidworks_envelopes(
        scan.sections(ctx)?
            .map(|section| (section.name(), section.payload())),
        ctx,
    )?;
    scan.solidworks = solidworks;
    Ok(scan)
}

/// Every marker hit is tried as a block first (the CRC gate is effectively
/// false-positive-free), then as a cache cell, then as a directory entry.


#[derive(Default)]
struct NativeMarkers {
    blocks: Vec<Block>,
    directory: Vec<DirectoryEntry>,
    cache_cells: Vec<CacheCell>,
}

fn walk_native_markers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut try_one_block: impl FnMut(usize) -> Result<Option<RawBlock>, CodecError>,
    mut try_one_cell: impl FnMut(usize) -> Result<Option<CacheCell>, CodecError>,
    mut try_one_directory: impl FnMut(usize) -> Result<Option<DirectoryEntry>, CodecError>,
) -> Result<NativeMarkers, CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), "scan SLDPRT native markers")?;
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
            i = block.offset + block_hdr::LEN + block.preamble_len + index_from_u32(block.comp_sz);
            ctx.reserve_vec(&mut blocks, 1, "admit SLDPRT block")?;
            ctx.charge_entities(1, "admit SLDPRT block")?;
            blocks.push(block.into_block(ctx)?);
            continue;
        }
        if let Some(cell) = try_one_cell(i)? {
            ctx.reserve_vec(&mut cache_cells, 1, "admit SLDPRT cache cell")?;
            ctx.charge_entities(1, "admit SLDPRT cache cell")?;
            cache_cells.push(cell);
        } else if let Some(entry) = try_one_directory(i)? {
            ctx.reserve_vec(&mut directory, 1, "admit SLDPRT directory entry")?;
            ctx.charge_entities(1, "admit SLDPRT directory entry")?;
            directory.push(entry);
        }
        i += 1;
    }
    Ok(NativeMarkers {
        blocks,
        directory,
        cache_cells,
    })
}

fn compound_stream(
    ctx: &DecodeContext<'_>,
    path: String,
    directory_id: u32,
    start_sector: u32,
    bytes: Vec<u8>,
    decoded_bytes: Option<Vec<u8>>,
) -> Result<CompoundStream, CodecError> {
    let semantic_payload = decoded_bytes.as_deref().unwrap_or(&bytes);
    let ps_streams = crate::parasolid::extract_streams_with_offsets(semantic_payload, ctx)?;
    let path = match cadmpeg_ir::StreamName::try_from(path) {
        Ok(path) => path,
        Err(_) => cadmpeg_ir::stream_name!("compound@").with_suffix(
            ctx,
            directory_id,
            "compose annotation stream name",
        )?,
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
    ctx: &DecodeContext<'_>,
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
    let mut envelope = root;
    let mut header = envelope
        .req_take_child(outer_hdr::LEN)
        .map_err(|error| error.during("read SLDPRT native outer header"))?;
    // discarded-value: the file ID precedes the required version field
    let _ = header.req_take(outer_hdr::VERSION)?;
    let version = header.req_u32_be()?;
    let NativeMarkers {
        blocks,
        directory,
        cache_cells,
    } = walk_native_markers(
        ctx,
        bytes,
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
fn compound_streams(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<Vec<CompoundStream>, CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    let mut streams = Vec::new();
    for entry in ctx.admit_iter(snapshot.entries(), "scan SLDPRT compound entries")? {
        let CompoundEntry::Stream(entry) = entry else {
            continue;
        };
        let view = snapshot.open(ctx, entry)?;
        let payload = ctx.copy_retained(view.window(), "retain SolidWorks CFB stream")?;
        let decoded = decode_wrapped_payload_budgeted(ctx, view)?;
        let mut path = String::new();
        ctx.try_reserve_retained_text(&mut path, entry.path().len(), "retain SLDPRT stream path")?;
        path.push_str(entry.path());
        let stream = compound_stream(
            ctx,
            path,
            entry.id().directory_id(),
            entry.start_sector(),
            payload,
            decoded,
        )?;
        ctx.reserve_vec(&mut streams, 1, "admit SLDPRT compound stream")?;
        streams.push(stream);
    }
    Ok(streams)
}

fn decode_wrapped_payload_budgeted(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
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
    fn into_block(self, ctx: &DecodeContext<'_>) -> Result<Block, CodecError> {
        let section = match self.section {
            Some(name) => match cadmpeg_ir::StreamName::try_from(name) {
                Ok(name) => BlockName::Named(name),
                Err(_) => BlockName::Anonymous(cadmpeg_ir::stream_name!("block@").with_suffix(
                    ctx,
                    self.offset,
                    "compose annotation stream name",
                )?),
            },
            None => BlockName::Anonymous(cadmpeg_ir::stream_name!("block@").with_suffix(
                ctx,
                self.offset,
                "compose annotation stream name",
            )?),
        };
        Ok(Block {
            offset: self.offset,
            type_id: self.type_id,
            comp_sz: self.comp_sz,
            section,
            family: self.family,
            payload: self.payload,
            ps_streams: self.ps_streams,
        })
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

    let comp = index_from_u32(comp_sz);
    let pre = index_from_u32(pre_sz);
    if comp == 0 {
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
    if inflated.len() != index_from_u32(frame.uncomp_sz) {
        return Ok(None);
    }
    ctx.charge_work(u64_from_index(inflated.len()), "validate SLDPRT block CRC")?;
    if crc32fast::hash(&inflated) != frame.crc {
        return Ok(None);
    }

    let payload_start = off + block_hdr::LEN + index_from_u32(frame.pre_sz);
    let preamble = bytes
        .get(off + block_hdr::LEN..payload_start)
        .unwrap_or(&[]);
    let section = nibble_swap_name_charged(ctx, preamble)?;
    // A Parasolid block is one from which a `PS\0\0` stream can be extracted (in
    // plain, wrapped, or nested form); otherwise fall back to a byte-signature
    // family label.
    let ps_streams = crate::parasolid::extract_streams_with_offsets(&inflated, ctx)?;
    let family = if ps_streams.is_empty() {
        payload_family(ctx, &inflated, "classify SLDPRT block payload")?
    } else {
        PayloadFamily::Parasolid
    };

    Ok(Some(RawBlock {
        offset: off,
        type_id: frame.type_id,
        comp_sz: frame.comp_sz,
        preamble_len: index_from_u32(frame.pre_sz),
        section,
        family,
        payload: inflated,
        ps_streams,
    }))
}

fn try_block_budgeted(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    off: usize,
) -> Result<Option<RawBlock>, CodecError> {
    let bytes = root.window();
    let Some((frame, payload_start, payload_end)) = read_block_frame(bytes, off) else {
        return Ok(None);
    };
    if index_from_u32(frame.uncomp_sz) > MAX_UNCOMP {
        return Err(ctx.refuse_codec_limit(
            "expand SLDPRT native block",
            u64_from_index(MAX_UNCOMP),
            u64::from(frame.uncomp_sz),
        ));
    }
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
    let Some(raw) = bytes.get(name_start..name_start + index_from_u32(name_len)) else {
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
    let Some(raw) = bytes.get(name_start..name_start + index_from_u32(name_len)) else {
        return Ok(None);
    };
    let Some(descriptor) = bytes
        .get(off + dir_ent::DESCRIPTOR..off + dir_ent::LEN)
        .and_then(|value| value.try_into().ok())
    else {
        return Ok(None);
    };
    let Some(trailer) = bytes
        .get(name_start + index_from_u32(name_len)..name_start + index_from_u32(name_len) + 6)
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
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    dialects: DialectLayers,
) -> Result<ContainerSummary, CodecError> {
    let mut entries = Vec::new();

    for b in ctx.admit_iter(&scan.blocks, "scan SLDPRT summarize values")? {
        ctx.charge_work(
            u64_from_index(b.payload.len()),
            "hash SLDPRT inventory payload",
        )?;
        ctx.charge_retained(64, "retain SLDPRT inventory digest")?;
        let mut attributes = BTreeMap::new();
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("offset"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", b.offset),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("type_id"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("0x{:08x}", b.type_id),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("family"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", b.family.label()),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("sha256"), "retain SLDPRT inventory key")?,
            sha256_hex(&b.payload),
            "collect SLDPRT inventory attributes",
        )?;
        if let Some(stream) = b.ps_streams.first() {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.format_retained(
                    format_args!("parasolid_schema"),
                    "retain SLDPRT inventory key",
                )?,
                ctx.format_retained(
                    format_args!("{}", stream.header.schema.value()),
                    "retain SLDPRT inventory value",
                )?,
                "collect SLDPRT inventory attributes",
            )?;
            ctx.insert_btree_map(
                &mut attributes,
                ctx.format_retained(
                    format_args!("parasolid_description"),
                    "retain SLDPRT inventory key",
                )?,
                ctx.format_retained(
                    format_args!("{}", stream.header.description),
                    "retain SLDPRT inventory value",
                )?,
                "collect SLDPRT inventory attributes",
            )?;
        }
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.format_retained(
                    format_args!("{}", b.section.source_stream().as_str()),
                    "retain SLDPRT inventory name",
                )?,
                role: ContainerRole::Block,
                storage: EntryStorage::Compressed {
                    method: CompressionMethod::Deflate,
                    stored: Some(u64::from(b.comp_sz)),
                    expanded: Some(u64_from_index(b.uncomp_sz())),
                },
                attributes,
            },
            "collect SLDPRT inventory entries",
        )?;
    }

    for d in ctx.admit_iter(&scan.directory, "scan SLDPRT summarize values")? {
        let mut attributes = BTreeMap::new();
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("offset"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", d.offset),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("type_id"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("0x{:08x}", d.type_id),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.format_retained(
                    format_args!("{}", d.name.as_str()),
                    "retain SLDPRT inventory name",
                )?,
                role: ContainerRole::DirectoryEntry,
                storage: EntryStorage::payload_only(VerbatimLabel::None, u64::from(d.size)),
                attributes,
            },
            "collect SLDPRT inventory entries",
        )?;
    }

    for c in ctx.admit_iter(&scan.cache_cells, "scan SLDPRT summarize values")? {
        let mut attributes = BTreeMap::new();
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("offset"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", c.offset),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("logical_len"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", c.logical_len),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.format_retained(
                    format_args!("{}", c.name.as_str()),
                    "retain SLDPRT inventory name",
                )?,
                role: ContainerRole::CacheCell,
                storage: EntryStorage::payload_only(VerbatimLabel::None, u64::from(c.logical_len)),
                attributes,
            },
            "collect SLDPRT inventory entries",
        )?;
    }

    for stream in ctx.admit_iter(&scan.compound_streams, "scan SLDPRT summarize values")? {
        let family = payload_family(ctx, &stream.payload, "classify SLDPRT inventory payload")?;
        ctx.charge_work(
            u64_from_index(stream.payload.len()),
            "hash SLDPRT inventory payload",
        )?;
        ctx.charge_retained(64, "retain SLDPRT inventory digest")?;
        let mut attributes = BTreeMap::new();
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("start_sector"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", stream.start_sector),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("sha256"), "retain SLDPRT inventory key")?,
            sha256_hex(&stream.payload),
            "collect SLDPRT inventory attributes",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            ctx.format_retained(format_args!("family"), "retain SLDPRT inventory key")?,
            ctx.format_retained(
                format_args!("{}", family.label()),
                "retain SLDPRT inventory value",
            )?,
            "collect SLDPRT inventory attributes",
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name: ctx.format_retained(
                    format_args!("{}", stream.path.as_str()),
                    "retain SLDPRT inventory name",
                )?,
                role: ContainerRole::CompoundStream,
                storage: EntryStorage::verbatim(
                    VerbatimLabel::Stored,
                    u64_from_index(stream.payload.len()),
                ),
                attributes,
            },
            "collect SLDPRT inventory entries",
        )?;
    }

    Ok(ContainerSummary::classified(
        dialects,
        if scan.compound_streams.is_empty() {
            cadmpeg_ir::ContainerKind::SldprtBlocks
        } else {
            cadmpeg_ir::ContainerKind::CompoundFileBinary
        },
        entries,
        Vec::new(),
        notes_charged(ctx, scan)?,
    ))
}

const NO_ACTIVE_PARASOLID_NOTE: &str =
    "no unique active Parasolid partition located; available B-rep sites remain decodable";

pub(crate) fn notes_charged(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Vec<String>, CodecError> {
    let mut notes = ctx.collection_vec(3, "collect SLDPRT container notes")?;
    notes.push(ctx.format_retained(
        format_args!(
            "outer version word: 0x{:08x}; {} CRC-validated block(s), {} tail-directory \
         entry/entries, {} cache-cell(s), {} compound stream(s)",
            scan.version,
            scan.blocks.len(),
            scan.directory.len(),
            scan.cache_cells.len(),
            scan.compound_streams.len()
        ),
        "retain SLDPRT container note",
    )?);
    notes.push(match active_parasolid_summary(ctx, scan)? {
        Some((name, size, schema)) => ctx.format_retained(
            format_args!(
                "active Parasolid B-rep candidate: {} ({} bytes, schema {})",
                name,
                size,
                schema.schema.value()
            ),
            "retain SLDPRT active site note",
        )?,
        None => ctx.format_retained(
            format_args!("{NO_ACTIVE_PARASOLID_NOTE}"),
            "retain SLDPRT active site note",
        )?,
    });
    notes.push(ctx.format_retained(
        format_args!(
            "Parasolid body streams supply the typed topology and analytic carriers used by decode"
        ),
        "retain SLDPRT geometry note",
    )?);
    Ok(notes)
}

pub(crate) fn active_parasolid_summary<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Option<(&'a str, usize, &'a crate::parasolid::StreamHeader)>, CodecError> {
    let Some(selected) = select_active_parasolid_site(ctx, scan)? else {
        return Ok(None);
    };
    Ok(Some((
        selected.section.source_stream().as_str(),
        selected.payload.len(),
        selected.header,
    )))
}

/// Test whether either outer envelope carries a framed Parasolid body stream.
pub(crate) fn has_parasolid_body_stream(ctx: &DecodeContext<'_>, scan: &ContainerScan) -> Result<bool, CodecError> {
    for section in scan.sections(ctx)? {
        for stream in ctx.admit_iter(section.ps_streams(), "scan SLDPRT framed body streams")? {
            if crate::parasolid::is_body_stream(ctx, &stream.header)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Option<ActiveParasolidSite<'a>>, CodecError> {
    let active_configuration = active_configuration_index(ctx, scan)?;
    let mut selected = None;
    for section in scan.sections(ctx)? {
        let name = section.name().unwrap_or("");
        let section_is_partition = contains_ascii_case_insensitive(name, "partition")
            && !contains_ascii_case_insensitive(name, "ghost")
            && !contains_ascii_case_insensitive(name, "deltas")
            && !contains_ascii_case_insensitive(name, "resolvedfeatures");
        let section_is_admissible = !contains_ascii_case_insensitive(name, "ghost")
            && !contains_ascii_case_insensitive(name, "deltas")
            && !contains_ascii_case_insensitive(name, "resolvedfeatures");
        let sole_body_stream = ctx.admit_iter(section.ps_streams(), "count SLDPRT section body streams")?
            .try_fold(0_usize, |count, stream| {
                Ok::<_, CodecError>(count + usize::from(crate::parasolid::is_body_stream(ctx, &stream.header)?))
            })?
            == 1;
        for stream in ctx.admit_iter(section.ps_streams(), "scan SLDPRT section body streams")? {
            if !crate::parasolid::is_body_stream(ctx, &stream.header)? {
                continue;
            }
            let description = &stream.header.description;
            if !section_is_admissible
                || contains_ascii_case_insensitive(description, "ghost")
                || contains_ascii_case_insensitive(description, "deltas")
                || !(contains_ascii_case_insensitive(description, "partition")
                    || sole_body_stream && section_is_partition)
                || match active_configuration {
                    Some(active) => section.name().map(|name| configuration_index(ctx, name)).transpose()?.flatten() != Some(active),
                    None => false,
                }
            {
                continue;
            }
            if selected.is_some() {
                return Ok(None);
            }
            selected = Some(ActiveParasolidSite {
                section,
                payload: &stream.payload,
                header: &stream.header,
            });
        }
    }
    Ok(selected)
}

pub(crate) fn configuration_index(ctx: &cadmpeg_core::decode::DecodeContext<'_>, section: &str) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let start = match ctx.admit_iter(section.as_bytes(), "scan SLDPRT configuration section marker")?
        .windows(std::num::NonZeroUsize::new(b"config-".len()).ok_or_else(|| CodecError::malformed("zero configuration marker width"))?)
        .position(|window| window.eq_ignore_ascii_case(b"config-")) { Some(value) => value, None => return Ok(None) }
        + b"config-".len();
    let digit_count = ctx.admit_iter(&section.as_bytes()[start..], "scan SLDPRT configuration section digits")?
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    Ok((digit_count > 0)
        .then(|| section[start..start + digit_count].parse().ok())
        .flatten())
}

pub(crate) fn active_configuration_index(ctx: &cadmpeg_core::decode::DecodeContext<'_>, scan: &ContainerScan) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    Ok::<_, cadmpeg_core::CodecError>(explicit_active_configuration_index(ctx, scan)?.or_else(|| {
        scan.solidworks
            .manifest_active_configuration
            .unique_ref()
            .map(|(index, _)| index)
    }))
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
    ctx: &DecodeContext<'_>,
    document: &roxmltree::Document<'_>,
    row: roxmltree::Node<'_, '_>,
    id: &str,
) -> Result<Option<String>, CodecError> {
    let direct = row.attribute("swName").filter(|value| !value.is_empty());
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
    name.map(|value| ctx.copy_retained_text(value, "retain SLDPRT manifest configuration name")).transpose()
}

fn explicit_active_configuration_index(ctx: &cadmpeg_core::decode::DecodeContext<'_>, scan: &ContainerScan<'_>) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let active = match active_configuration_name_ref(scan) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
    let indices = match ctx.get_btree_map(&(scan.solidworks.configuration_source_indices), active, "look up SLDPRT ordered key")? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
    Ok::<_, cadmpeg_core::CodecError>((indices.len() == 1).then(|| indices[0]))
}

pub(crate) fn active_configuration_name_ref<'a>(scan: &'a ContainerScan<'_>) -> Option<&'a str> {
    scan.solidworks
        .manifest_active_configuration
        .unique_ref()
        .and_then(|(_, name)| name)
        .or_else(|| {
            (scan.solidworks.configuration_names.len() == 1)
                .then(|| {
                    scan.solidworks
                        .configuration_names
                        .iter()
                        .next()
                        .map(String::as_str)
                })
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


pub(crate) fn xml_text_charged<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Option<EnvelopeText<'ctx>>, CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), operation)?;
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) {
        let utf16 = &bytes[2..];
        let (text, scope) =
            ctx.utf16le_lossy_scoped_text(utf16, utf16.len() / 2, false, operation)?;
        Ok(Some(EnvelopeText {
            text,
            _scope: Some(scope),
        }))
    } else {
        let Ok(source) = std::str::from_utf8(bytes) else {
            return Ok(None);
        };
        let (mut text, scope) = ctx.scoped_string(source.len(), operation)?;
        text.push_str(source);
        Ok(Some(EnvelopeText {
            text,
            _scope: Some(scope),
        }))
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
    ctx: &DecodeContext<'_>,
) -> Result<SolidWorksEnvelopeScan, CodecError> {
    let mut scan = SolidWorksEnvelopeScan::default();
    for (section, payload) in sections {
        let Some(text) = xml_text_charged(ctx, payload, "materialize SLDPRT XML text")? else {
            continue;
        };
        let admitted = match ctx.parse_xml(&text.text, "SLDPRT envelope XML tree") {
            Ok(tree) => tree,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => continue,
        };
        let document = admitted.document();
        let root = document.root_element();
        if is_features_manifest_name(section) && root.tag_name().name() == "swSolidWorks" {
            scan.manifest_active_configuration
                .merge(manifest_active_configuration_in(ctx, document)?);
        }
        if root.tag_name().name().contains("Keywords") {
            for configuration in document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
            {
                let Some(name) = configuration.attribute("Name") else {
                    continue;
                };
                let Some(value) = configuration.attribute("SourceIndex") else {
                    continue;
                };
                if value.is_empty() || !ctx.admit_iter(value, "scan SLDPRT configuration source index digits")?.all(|character| character.is_ascii_digit()) {
                    continue;
                }
                let Some(index) = value.parse::<usize>().ok() else {
                    continue;
                };
                if let Some(indices) = ctx.get_mut_btree_map(&mut (scan.configuration_source_indices), name, "look up mutable SLDPRT ordered key")? {
                    ctx.reserve_vec(indices, 1, "collect SLDPRT configuration source indices")?;
                    indices.push(index);
                } else {
                    ctx.charge_collection_items(1, "collect SLDPRT configuration source names")?;
                    let name =
                        ctx.copy_retained_text(name, "retain SLDPRT configuration source name")?;
                    let mut indices = Vec::new();
                    ctx.reserve_vec(&mut indices, 1, "collect SLDPRT configuration source indices")?;
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
                ctx.charge_collection_items(1, "collect SLDPRT configuration names")?;
                let name = ctx.copy_retained_text(name, "retain SLDPRT configuration name")?;
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
            if !ctx.admit_iter(slot, "scan SLDPRT configuration slot digits")?.all(|character| character.is_ascii_digit()) {
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
                        ctx.charge_collection_items(1, "collect SLDPRT configuration attributes")?;
                    }
                    source_attributes.insert((slot, target), value);
                }
            }
        }
        let mut configuration_attributes = BTreeMap::new();
        for ((slot, target), value) in source_attributes {
            let length = "sw_configuration_".len() + slot.len() + 1 + target.len();
            let mut key = String::new();
            ctx.try_reserve_retained_text(&mut key, length, "retain SLDPRT configuration key")?;
            key.push_str("sw_configuration_");
            key.push_str(slot);
            key.push('_');
            key.push_str(target);
            let value = ctx.copy_retained_text(value, "retain SLDPRT configuration value")?;
            ctx.charge_collection_items(1, "retain SLDPRT configuration attribute")?;
            configuration_attributes.insert(key, value);
        }
        scan.first = Some(SolidWorksEnvelope {
            sw_version: root.attribute("swVersion").map(|value| ctx.copy_retained_text(value, "retain SLDPRT version")).transpose()?,
            creation_time: root.attribute("swCreationTime").map(|value| ctx.copy_retained_text(value, "retain SLDPRT creation time")).transpose()?,
            path: root.attribute("swPath").map(|value| ctx.copy_retained_text(value, "retain SLDPRT path")).transpose()?,
            model_name: model.and_then(|node| node.attribute("swName")).map(|value| ctx.copy_retained_text(value, "retain SLDPRT model name")).transpose()?,
            configuration_name: model.and_then(|node| node.attribute("swConfigurationName")).map(|value| ctx.copy_retained_text(value, "retain SLDPRT envelope configuration name")).transpose()?,
            configuration_attributes,
        });
    }
    Ok(scan)
}

fn manifest_active_configuration_in(
    ctx: &DecodeContext<'_>,
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
    let Some(index) = (!id.is_empty() && ctx.admit_iter(id, "scan SLDPRT active configuration id digits")?.all(|character| character.is_ascii_digit()))
        .then(|| id.parse::<usize>().ok())
        .flatten()
    else {
        return Ok(ManifestActiveConfiguration::Ambiguous);
    };
    Ok(ManifestActiveConfiguration::Unique(
        index,
        manifest_configuration_name(ctx, document, row, id)?,
    ))
}

/// Returns the first parsed `swSolidWorks` envelope even if an attribute is absent.
pub(crate) fn first_solidworks_envelope<'a>(
    ctx: &DecodeContext<'_>,
    payloads: impl IntoIterator<Item = &'a [u8]>,
) -> Result<Option<SolidWorksEnvelope>, CodecError> {
    Ok(scan_solidworks_envelopes(
        payloads.into_iter().map(|payload| (None, payload)),
        ctx,
    )?.first)
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
