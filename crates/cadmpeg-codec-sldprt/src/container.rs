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

/// Classification words a section name or stream description contains.
///
/// Each word matches anywhere in the text, ignoring ASCII case. One admitted
/// pass finds every word, so a text is classified once, when it is admitted,
/// and each later test reads one bit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NameWords(u8);

impl NameWords {
    const PARTITION: u8 = 1;
    const DELTAS: u8 = 2;
    const GHOST: u8 = 4;
    const RESOLVED_FEATURES: u8 = 8;

    /// A text that contains only the word `partition`.
    #[cfg(test)]
    pub(crate) const PARTITION_ONLY: Self = Self(Self::PARTITION);

    /// A text that contains only the word `resolvedfeatures`.
    #[cfg(test)]
    pub(crate) const RESOLVED_FEATURES_ONLY: Self = Self(Self::RESOLVED_FEATURES);

    /// Classify `text` in one pass. Each position compares a fixed number of
    /// bytes against each word.
    pub(crate) fn of(
        ctx: &DecodeContext<'_>,
        text: &str,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let bytes = text.as_bytes();
        let mut words = 0;
        for start in ctx.admit_iter(0..bytes.len(), operation)? {
            let rest = &bytes[start..];
            for (word, bit) in [
                (&b"partition"[..], Self::PARTITION),
                (b"deltas", Self::DELTAS),
                (b"ghost", Self::GHOST),
                (b"resolvedfeatures", Self::RESOLVED_FEATURES),
            ] {
                if starts_with_word(rest, word) {
                    words |= bit;
                }
            }
        }
        Ok(Self(words))
    }

    pub(crate) fn partition(self) -> bool {
        self.0 & Self::PARTITION != 0
    }

    pub(crate) fn deltas(self) -> bool {
        self.0 & Self::DELTAS != 0
    }

    pub(crate) fn ghost(self) -> bool {
        self.0 & Self::GHOST != 0
    }

    pub(crate) fn resolved_features(self) -> bool {
        self.0 & Self::RESOLVED_FEATURES != 0
    }
}

/// Whether `bytes` begins with `word`, ignoring ASCII case.
/// Each word is a literal of at most sixteen bytes, so the test is fixed work.
fn starts_with_word(bytes: &[u8], word: &[u8]) -> bool {
    bytes
        .get(..word.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(word))
}

/// The text after the last `/` of `path`, or all of it. The search from the
/// end charges each byte it visits.
fn path_basename<'a>(
    ctx: &DecodeContext<'_>,
    path: &'a str,
    operation: &'static str,
) -> Result<&'a str, CodecError> {
    Ok(
        match ctx.rposition_by(path.as_bytes(), |byte| Ok(*byte == b'/'), operation)? {
            Some(slash) => &path[slash + 1..],
            None => path,
        },
    )
}

/// Classify a decompressed block payload by signature.
///
/// Unknown signatures return [`PayloadFamily::Unknown`].
pub(crate) fn payload_family(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    operation: &'static str,
) -> Result<PayloadFamily, CodecError> {
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

/// Decode a nibble-swapped section name. Every swapped byte is printable
/// ASCII, so each one is one character of the name.
fn nibble_swap_name_charged(
    ctx: &DecodeContext<'_>,
    raw: &[u8],
) -> Result<Option<String>, CodecError> {
    if !ctx.all_by(
        raw,
        |byte| Ok((0x20..0x7f).contains(&byte.rotate_left(4))),
        "validate SLDPRT section name",
    )? {
        return Ok(None);
    }
    let mut name = String::new();
    ctx.try_reserve_retained_text(&mut name, raw.len(), "retain SLDPRT section name")?;
    for byte in ctx.admit_iter(raw, "retain SLDPRT section name")? {
        name.push(char::from(byte.rotate_left(4)));
    }
    Ok(Some(name))
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
    /// Classification words of the section name.
    pub(crate) name_words: NameWords,
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
    /// Classification words of the stream path.
    pub(crate) name_words: NameWords,
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

    /// Classification words of [`Self::name`]; a block without a section
    /// name has none.
    pub(crate) fn name_words(self) -> NameWords {
        match self {
            Self::Block(block) => block.name_words,
            Self::Compound(stream) => stream.name_words,
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
    /// Every section, with the whole traversal admitted before the first.
    pub(crate) fn sections(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = Section<'_>>, CodecError> {
        let blocks = ctx.admit_iter(&self.blocks, "scan SLDPRT block sections")?;
        let streams = ctx.admit_iter(&self.compound_streams, "scan SLDPRT compound sections")?;
        Ok(blocks
            .map(Section::Block)
            .chain(streams.map(Section::Compound)))
    }

    /// Every section, one fixed step at a time, for a search that charges
    /// each step it takes.
    pub(crate) fn section_steps(&self) -> impl Iterator<Item = Section<'_>> {
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
pub(crate) fn looks_like_sldprt(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    if prefix.starts_with(&COMPOUND_FILE_MAGIC) {
        let (probe, _storage) =
            CompoundPrefixProbe::inspect_with_context(ctx, View::over_retained(prefix))?;
        let Some(paths) = probe.paths() else {
            return Ok(false);
        };
        return ctx.any_by(
            paths,
            |path| {
                Ok(
                    path_basename(ctx, path, "scan SolidWorks directory basename")?
                        .eq_ignore_ascii_case("ISolidWorksInformation"),
                )
            },
            "compare SolidWorks directory evidence",
        );
    }
    let Some(body) = prefix.get(outer_hdr::LEN..) else {
        return Ok(false);
    };
    ctx.any_by(
        body.windows(MARKER.len()),
        |window| Ok(window == MARKER),
        "scan SolidWorks detection marker",
    )
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
        scan.section_steps()
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
    let mut blocks = Vec::new();
    let mut directory = Vec::new();
    let mut cache_cells = Vec::new();
    let mut i = outer_hdr::LEN;
    // Each search step compares one fixed-width window; bytes a block spans
    // are skipped, not searched.
    while let Some(relative) = match bytes.get(i..) {
        Some(rest) => ctx.position_by(
            rest.windows(MARKER.len()),
            |window| Ok(window == MARKER),
            "scan SLDPRT native markers",
        )?,
        None => None,
    } {
        i += relative;
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
    let name_words = NameWords::of(ctx, path.as_str(), "classify SLDPRT section name")?;
    Ok(CompoundStream {
        path,
        name_words,
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
        let path = ctx.copy_retained_text(entry.path(), "retain SLDPRT stream path")?;
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
        let name_words = match &section {
            BlockName::Named(name) => {
                NameWords::of(ctx, name.as_str(), "classify SLDPRT section name")?
            }
            BlockName::Anonymous(_) => NameWords::default(),
        };
        Ok(Block {
            offset: self.offset,
            type_id: self.type_id,
            comp_sz: self.comp_sz,
            section,
            name_words,
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
pub(crate) fn has_parasolid_body_stream(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<bool, CodecError> {
    ctx.any_by(
        scan.section_steps(),
        |section| {
            ctx.any_by(
                section.ps_streams(),
                |stream| Ok(stream.header.is_body_stream()),
                "scan SLDPRT framed body streams",
            )
        },
        "scan SLDPRT block sections",
    )
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
    const OPERATION: &str = "scan SLDPRT section body streams";
    let active_configuration = active_configuration_index(ctx, scan)?;
    let mut selected = None;
    let mut sections = scan.section_steps();
    while let Some(section) = ctx.next_charged(&mut sections, "scan SLDPRT block sections")? {
        let name = section.name_words();
        if name.ghost() || name.deltas() || name.resolved_features() {
            continue;
        }
        let sole_body_stream = ctx
            .admit_iter(section.ps_streams(), "count SLDPRT section body streams")?
            .filter(|stream| stream.header.is_body_stream())
            .count()
            == 1;
        // The section's configuration index, read at most once and only for
        // a stream every other test admits.
        let mut configuration = None;
        let mut streams = section.ps_streams().iter();
        while let Some(stream) = ctx.next_charged(&mut streams, OPERATION)? {
            let description = stream.header.words;
            if !stream.header.is_body_stream()
                || description.ghost()
                || description.deltas()
                || !(description.partition() || sole_body_stream && name.partition())
            {
                continue;
            }
            if let Some(active) = active_configuration {
                let index = match configuration {
                    Some(index) => index,
                    None => *configuration.insert(
                        section
                            .name()
                            .map(|name| configuration_index(ctx, name))
                            .transpose()?
                            .flatten(),
                    ),
                };
                if index != Some(active) {
                    continue;
                }
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

/// The configuration index a section name states after `config-`, ignoring
/// ASCII case.
pub(crate) fn configuration_index(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    section: &str,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    const MARKER: &[u8; 7] = b"config-";
    let bytes = section.as_bytes();
    let Some(marker) = ctx.position_by(
        bytes.windows(MARKER.len()),
        |window| Ok(window.eq_ignore_ascii_case(MARKER)),
        "scan SLDPRT configuration section marker",
    )?
    else {
        return Ok(None);
    };
    let start = marker + MARKER.len();
    let digits = &bytes[start..];
    let digit_count = ctx
        .position_by(
            digits,
            |byte| Ok(!byte.is_ascii_digit()),
            "scan SLDPRT configuration section digits",
        )?
        .unwrap_or(digits.len());
    if digit_count == 0 {
        return Ok(None);
    }
    Ok(ctx
        .parse_text::<usize>(
            &section[start..start + digit_count],
            "parse SLDPRT configuration section index",
        )?
        .ok())
}

pub(crate) fn active_configuration_index(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    Ok::<_, cadmpeg_core::CodecError>(explicit_active_configuration_index(ctx, scan)?.or_else(
        || {
            scan.solidworks
                .manifest_active_configuration
                .unique_ref()
                .map(|(index, _)| index)
        },
    ))
}

/// Return the active configuration's unique manifest identity, when one
/// manifest row provides it. A manifest with zero or several `YES` rows is
/// deliberately not an index source.
#[cfg(test)]
fn manifest_active_configuration(scan: &ContainerScan<'_>) -> Option<(usize, Option<String>)> {
    scan.solidworks.manifest_active_configuration.unique()
}

fn is_features_manifest_name(
    ctx: &DecodeContext<'_>,
    name: Option<&str>,
) -> Result<bool, CodecError> {
    let Some(name) = name else {
        return Ok(false);
    };
    Ok(
        path_basename(ctx, name, "scan SLDPRT manifest section name")?
            .eq_ignore_ascii_case("Features"),
    )
}

/// The value of the attribute of `node` named `name` outside any namespace,
/// the attribute `roxmltree::Node::attribute` finds. Each visited attribute is
/// charged; `name` is a literal, so each comparison is fixed work.
fn unqualified_attribute<'a>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'a, '_>,
    name: &str,
    operation: &'static str,
) -> Result<Option<&'a str>, CodecError> {
    Ok(ctx
        .find_by(
            node.attributes(),
            |attribute| Ok(attribute.namespace().is_none() && attribute.name() == name),
            operation,
        )?
        .map(|attribute| attribute.value()))
}

/// The first element below `root`, in document order, that `matches`
/// accepts. Each visited node is charged before `matches` reads it.
fn find_element<'a, 'input>(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'a, 'input>,
    mut matches: impl FnMut(roxmltree::Node<'a, 'input>) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    ctx.find_by(
        root.descendants(),
        |node| Ok(node.is_element() && matches(*node)?),
        operation,
    )
}

/// Resolve a manifest row to its configuration name without depending on XML
/// namespace prefixes or on the order of the model rows.
fn manifest_configuration_name(
    ctx: &DecodeContext<'_>,
    document: &roxmltree::Document<'_>,
    row: roxmltree::Node<'_, '_>,
    id: &str,
) -> Result<Option<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT manifest configuration name";
    let model_with = |attribute: &str, value: &str| {
        find_element(
            ctx,
            document.root(),
            |node| {
                Ok(node.tag_name().name() == "swModel"
                    && match unqualified_attribute(ctx, node, attribute, OPERATION)? {
                        Some(stated) => {
                            ctx.equal_bytes(stated.as_bytes(), value.as_bytes(), OPERATION)?
                        }
                        None => false,
                    })
            },
            OPERATION,
        )
    };
    let name = match ctx
        .xml_attribute(row, "swName", OPERATION)?
        .filter(|value| !value.is_empty())
    {
        Some(name) => Some(name),
        None => {
            let referenced = match unqualified_attribute(ctx, row, "swModelRef", OPERATION)? {
                Some(reference) => model_with("id", reference)?,
                None => None,
            };
            let model = match referenced {
                Some(model) => Some(model),
                None => model_with("swConfigurationId", id)?,
            };
            match model {
                Some(model) => ctx
                    .xml_attribute(model, "swConfigurationName", OPERATION)?
                    .filter(|value| !value.is_empty()),
                None => None,
            }
        }
    };
    name.map(|value| ctx.copy_retained_text(value, "retain SLDPRT manifest configuration name"))
        .transpose()
}

fn explicit_active_configuration_index(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let Some(active) = active_configuration_name_ref(scan) else {
        return Ok(None);
    };
    let Some(indices) = ctx.get_btree_map(
        &scan.solidworks.configuration_source_indices,
        active,
        "look up SLDPRT ordered key",
    )?
    else {
        return Ok(None);
    };
    Ok(match indices.as_slice() {
        [index] => Some(*index),
        _ => None,
    })
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

/// XML text of one section: borrowed when the payload is UTF-8, decoded into
/// scoped storage when it is UTF-16.
pub(crate) struct EnvelopeText<'ctx, 'bytes> {
    text: std::borrow::Cow<'bytes, str>,
    _scope: Option<ScopedReservation<'ctx>>,
}

impl EnvelopeText<'_, '_> {
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
}

pub(crate) fn xml_text_charged<'ctx, 'bytes>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'bytes [u8],
    operation: &'static str,
) -> Result<Option<EnvelopeText<'ctx, 'bytes>>, CodecError> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if let Some(utf16) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (text, scope) =
            ctx.utf16le_lossy_scoped_text(utf16, utf16.len() / 2, false, operation)?;
        return Ok(Some(EnvelopeText {
            text: std::borrow::Cow::Owned(text),
            _scope: Some(scope),
        }));
    }
    Ok(ctx
        .validate_utf8(bytes, operation)?
        .ok()
        .map(|text| EnvelopeText {
            text: std::borrow::Cow::Borrowed(text),
            _scope: None,
        }))
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
    fn merge(&mut self, ctx: &DecodeContext<'_>, current: Self) -> Result<(), CodecError> {
        match (&mut *self, current) {
            (_, Self::Absent) | (Self::Ambiguous, _) => {}
            (_, Self::Ambiguous) => *self = Self::Ambiguous,
            (Self::Absent, current @ Self::Unique(..)) => *self = current,
            (Self::Unique(previous_index, previous_name), Self::Unique(index, mut name)) => {
                if *previous_index != index
                    || matches!(
                        (previous_name.as_deref(), name.as_deref()),
                        (Some(previous), Some(current)) if !ctx.equal(previous, current, "compare SLDPRT active configuration names")?
                    )
                {
                    *self = Self::Ambiguous;
                } else if previous_name.is_none() {
                    *previous_name = name.take();
                }
            }
        }
        Ok(())
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

/// Whether `value` is non-empty and every byte is an ASCII digit.
fn is_decimal(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(!value.is_empty()
        && ctx.all_by(
            value.as_bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            operation,
        )?)
}

fn scan_solidworks_envelopes<'a>(
    sections: impl IntoIterator<Item = (Option<&'a str>, &'a [u8])>,
    ctx: &DecodeContext<'_>,
) -> Result<SolidWorksEnvelopeScan, CodecError> {
    const ATTRIBUTES: &str = "read SLDPRT envelope attributes";
    let mut scan = SolidWorksEnvelopeScan::default();
    let mut sections = sections.into_iter();
    while let Some((section, payload)) =
        ctx.next_charged(&mut sections, "scan SLDPRT envelope sections")?
    {
        let Some(text) = xml_text_charged(ctx, payload, "materialize SLDPRT XML text")? else {
            continue;
        };
        let admitted = match ctx.parse_xml(text.as_str(), "SLDPRT envelope XML tree") {
            Ok(tree) => tree,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => continue,
        };
        let document = admitted.document();
        let root = ctx.xml_root_element(document, "find SLDPRT envelope root")?;
        let root_name = root.tag_name().name();
        let is_envelope = root_name == "swSolidWorks";
        if is_envelope && is_features_manifest_name(ctx, section)? {
            scan.manifest_active_configuration
                .merge(ctx, manifest_active_configuration_in(ctx, document)?)?;
        }
        if ctx.contains_text(root_name, "Keywords", "classify SLDPRT envelope root")? {
            let mut nodes = document.descendants();
            while let Some(configuration) =
                ctx.next_charged(&mut nodes, "scan SLDPRT keyword configurations")?
            {
                if !configuration.is_element() || configuration.tag_name().name() != "Configuration"
                {
                    continue;
                }
                let Some(name) = unqualified_attribute(ctx, configuration, "Name", ATTRIBUTES)?
                else {
                    continue;
                };
                let Some(value) =
                    unqualified_attribute(ctx, configuration, "SourceIndex", ATTRIBUTES)?
                else {
                    continue;
                };
                if !is_decimal(ctx, value, "scan SLDPRT configuration source index digits")? {
                    continue;
                }
                let Ok(index) =
                    ctx.parse_text::<usize>(value, "parse SLDPRT configuration source index")?
                else {
                    continue;
                };
                if let Some(indices) = ctx.get_mut_btree_map(
                    &mut (scan.configuration_source_indices),
                    name,
                    "look up mutable SLDPRT ordered key",
                )? {
                    ctx.push_vec(
                        indices,
                        index,
                        "collect SLDPRT configuration source indices",
                    )?;
                } else {
                    let name =
                        ctx.copy_retained_text(name, "retain SLDPRT configuration source name")?;
                    let mut indices =
                        ctx.collection_vec(1, "collect SLDPRT configuration source indices")?;
                    indices.push(index);
                    ctx.insert_btree_map(
                        &mut (scan.configuration_source_indices),
                        name,
                        indices,
                        "collect SLDPRT configuration source names",
                    )?;
                }
            }
        }
        if !is_envelope {
            continue;
        }
        let mut nodes = root.descendants();
        while let Some(model) = ctx.next_charged(&mut nodes, "scan SLDPRT envelope models")? {
            if !model.has_tag_name("swModel") {
                continue;
            }
            let Some(name) = unqualified_attribute(ctx, model, "swConfigurationName", ATTRIBUTES)?
            else {
                continue;
            };
            if !ctx.contains_btree_set(
                &scan.configuration_names,
                name,
                "look up SLDPRT configuration names",
            )? {
                let name = ctx.copy_retained_text(name, "retain SLDPRT configuration name")?;
                ctx.insert_btree_set(
                    &mut scan.configuration_names,
                    name,
                    "collect SLDPRT configuration names",
                )?;
            }
        }
        if scan.first.is_some() {
            continue;
        }
        let model = find_element(
            ctx,
            root,
            |node| Ok(node.has_tag_name("swModel")),
            "find SLDPRT envelope model",
        )?;
        let mut source_attributes_storage =
            ctx.reserve_scoped(0, "SLDPRT temporary configuration attributes")?;
        let mut source_attributes = BTreeMap::new();
        let mut nodes = root.descendants();
        while let Some(configuration) =
            ctx.next_charged(&mut nodes, "scan SLDPRT envelope configurations")?
        {
            if !configuration.has_tag_name("swConfiguration") {
                continue;
            }
            let Some(slot) = unqualified_attribute(ctx, configuration, "swID", ATTRIBUTES)? else {
                continue;
            };
            if !ctx.all_by(
                slot.as_bytes(),
                |byte| Ok(byte.is_ascii_digit()),
                "scan SLDPRT configuration slot digits",
            )? {
                continue;
            }
            for (source, target) in [
                ("swConfigurationNeedsUpdate", "needs_update"),
                ("swMostRecentConfiguration", "most_recent"),
                ("swConfigurationFlags", "flags"),
                ("swConfigurationAlternateName", "alternate_name"),
            ] {
                if let Some(value) = unqualified_attribute(ctx, configuration, source, ATTRIBUTES)?
                {
                    source_attributes_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut source_attributes,
                            (slot, target),
                            value,
                            "collect SLDPRT configuration attributes",
                        )
                    })?;
                }
            }
        }
        let mut configuration_attributes = BTreeMap::new();
        for ((slot, target), value) in
            ctx.admit_iter(source_attributes, "retain SLDPRT configuration attributes")?
        {
            let key = ctx.format_retained(
                format_args!("sw_configuration_{slot}_{target}"),
                "retain SLDPRT configuration key",
            )?;
            let value = ctx.copy_retained_text(value, "retain SLDPRT configuration value")?;
            ctx.insert_btree_map(
                &mut (configuration_attributes),
                key,
                value,
                "retain SLDPRT configuration attribute",
            )?;
        }
        let retained_attribute = |node: Option<roxmltree::Node<'_, '_>>,
                                  attribute: &str,
                                  operation: &'static str|
         -> Result<Option<String>, CodecError> {
            match node {
                Some(node) => ctx
                    .xml_attribute(node, attribute, ATTRIBUTES)?
                    .map(|value| ctx.copy_retained_text(value, operation))
                    .transpose(),
                None => Ok(None),
            }
        };
        scan.first = Some(SolidWorksEnvelope {
            sw_version: retained_attribute(Some(root), "swVersion", "retain SLDPRT version")?,
            creation_time: retained_attribute(
                Some(root),
                "swCreationTime",
                "retain SLDPRT creation time",
            )?,
            path: retained_attribute(Some(root), "swPath", "retain SLDPRT path")?,
            model_name: retained_attribute(model, "swName", "retain SLDPRT model name")?,
            configuration_name: retained_attribute(
                model,
                "swConfigurationName",
                "retain SLDPRT envelope configuration name",
            )?,
            configuration_attributes,
        });
    }
    Ok(scan)
}

fn manifest_active_configuration_in(
    ctx: &DecodeContext<'_>,
    document: &roxmltree::Document<'_>,
) -> Result<ManifestActiveConfiguration, CodecError> {
    const ATTRIBUTES: &str = "read SLDPRT manifest configuration attributes";
    let mut any_configuration = false;
    let mut active = None;
    let mut nodes = document.descendants();
    while let Some(row) = ctx.next_charged(&mut nodes, "scan SLDPRT manifest configurations")? {
        if !row.is_element() || row.tag_name().name() != "swConfiguration" {
            continue;
        }
        any_configuration = true;
        if unqualified_attribute(ctx, row, "swMostRecentConfiguration", ATTRIBUTES)? == Some("YES")
        {
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
    let Some(id) = unqualified_attribute(ctx, row, "swID", ATTRIBUTES)? else {
        return Ok(ManifestActiveConfiguration::Ambiguous);
    };
    if !is_decimal(ctx, id, "scan SLDPRT active configuration id digits")? {
        return Ok(ManifestActiveConfiguration::Ambiguous);
    }
    let Ok(index) = ctx.parse_text::<usize>(id, "parse SLDPRT active configuration id")? else {
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
    Ok(scan_solidworks_envelopes(payloads.into_iter().map(|payload| (None, payload)), ctx)?.first)
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
