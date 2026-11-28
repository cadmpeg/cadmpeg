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

use cadmpeg_container::compound::{
    CompoundDeviation, CompoundEntry, CompoundPrefixProbe, CompoundSnapshot,
};
use cadmpeg_container::compression::{inflate_deflate_owned, inflate_zlib_member_owned};
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{
    index_from_u32, u64_from_index, ArenaClass, DecodeContext, ExpandSpec, View,
};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpPart, Part};
use cadmpeg_ir::ContainerSummary;

mod configuration_bindings;
pub(crate) mod native_layout;

use crate::layout::block_frame_header as block_hdr;
use crate::layout::cache_cell_header as cache_hdr;
use crate::layout::outer_header as outer_hdr;
use crate::layout::tail_directory_entry as dir_ent;
use crate::layout::zlb_wrapper_header as zlb_hdr;

/// Marker shared by block, cache-cell, and directory frames.
pub(crate) const MARKER: [u8; 6] = block_hdr::MARKER_VALUE;
/// The stated `version` of the native outer header (spec §1.1).
const NATIVE_VERSION: u32 = 4;

/// Resource ceiling on the expansion of one native block.
// resource-cap: MAX_UNCOMP = 2
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
        || payload.starts_with(b"\x85<?xml")
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

fn nibble_swap_name(raw: &[u8]) -> Option<String> {
    if !raw
        .iter()
        .all(|byte| (0x20..0x7f).contains(&byte.rotate_left(4)))
    {
        return None;
    }
    String::from_utf8(raw.iter().map(|byte| byte.rotate_left(4)).collect()).ok()
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
    /// The legacy zlib probe stopped at its work limit in this block.
    pub(crate) ps_probe_capped: bool,
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
    /// ZIP compressed size.
    pub(crate) size: u32,
    /// Decoded section name.
    pub(crate) name: String,
    /// Per-entry descriptor bytes at frame offset +26.
    pub(crate) descriptor: [u8; 14],
}

/// One cache-cell section-index entry.
#[derive(Debug, Clone)]
pub(crate) struct CacheCell {
    /// Byte offset of the marker.
    pub(crate) offset: usize,
    /// The logical cell size `L`.
    pub(crate) logical_len: u32,
    pub(crate) physical_range: Option<std::ops::Range<usize>>,
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
    /// The legacy zlib probe stopped at its work limit in this stream.
    pub(crate) ps_probe_capped: bool,
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
    /// Parts the scan could not read (spec §3.3), in file order.
    pub(crate) given_up: Vec<GivenUpPart>,
    /// Noncanonical CFB encodings the scan read past, each naming its stream.
    pub(crate) noncanonical: Vec<String>,
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

    /// The member's bytes as the file stores them, before `__ZLB` inflation.
    pub(crate) fn stored_payload(self) -> &'a [u8] {
        match self {
            Self::Block(block) => &block.payload,
            Self::Compound(stream) => &stream.payload,
        }
    }

    pub(crate) fn payload(self) -> &'a [u8] {
        match self {
            Self::Block(block) => &block.payload,
            Self::Compound(stream) => stream.decoded_payload.as_deref().unwrap_or(&stream.payload),
        }
    }

    pub(crate) fn xml_payload(self) -> Option<&'a [u8]> {
        if let Self::Compound(stream) = self {
            if stream
                .path
                .as_str()
                .rsplit_once("__")
                .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case("ZLB"))
                && stream.decoded_payload.is_none()
            {
                return None;
            }
        }
        Some(self.payload())
    }

    pub(crate) fn ps_streams(self) -> &'a [crate::parasolid::ExtractedStream] {
        match self {
            Self::Block(block) => &block.ps_streams,
            Self::Compound(stream) => &stream.ps_streams,
        }
    }

    pub(crate) fn ps_probe_capped(self) -> bool {
        match self {
            Self::Block(block) => block.ps_probe_capped,
            Self::Compound(stream) => stream.ps_probe_capped,
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

pub(crate) const COMPOUND_FILE_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
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
        let probe = CompoundPrefixProbe::inspect(ctx, View::over_retained(prefix))?;
        if let Some(paths) = probe.paths() {
            for path in paths {
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
    let mut header = View::over_retained(prefix);
    let version = header
        .req_take(outer_hdr::VERSION)
        .and_then(|_| header.req_u32_be())
        .ok();
    if version != Some(NATIVE_VERSION) {
        return Ok(false);
    }
    Ok(prefix[outer_hdr::LEN..]
        .windows(MARKER.len())
        .any(|w| w == MARKER))
}

fn completed_scan<'a>(
    ctx: &DecodeContext<'_>,
    source_image: &'a [u8],
    version: u32,
    native: NativeMarkers,
    compound: CompoundStreams,
) -> Result<ContainerScan<'a>, CodecError> {
    let NativeMarkers {
        blocks,
        directory,
        cache_cells,
        given_up,
    } = native;
    let CompoundStreams {
        streams: compound_streams,
        given_up: compound_given_up,
        noncanonical,
    } = compound;
    let mut scan = ContainerScan {
        source_image,
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams,
        solidworks: SolidWorksEnvelopeScan::default(),
        given_up: given_up.into_iter().chain(compound_given_up).collect(),
        noncanonical,
    };
    let solidworks = scan_solidworks_envelopes(
        scan.sections().filter_map(|section| {
            section
                .xml_payload()
                .map(|payload| (section.name(), payload))
        }),
        ctx,
        "SLDPRT envelope XML tree",
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
    given_up: Vec<GivenUpPart>,
}

/// The part a CFB stream is: opened, retained and ZLB-unwrapped.
pub(crate) const COMPOUND_STREAM_PART: &str = "compound-stream";
/// The part a native block frame is: inflated and checked.
pub(crate) const NATIVE_BLOCK_PART: &str = "native-block";
/// The part the Parasolid streams of one owner are: extracted together.
pub(crate) const PARASOLID_STREAMS_PART: &str = "parasolid-streams";
/// The part one ZLB-wrapped zlib member of an owner is: inflated to its
/// declared size.
pub(crate) const WRAPPED_MEMBER_PART: &str = "wrapped-member";

/// The classes of content a `SolidWorks` section's bytes can hold.
const SECTION_HOLDS: [ArenaClass; 2] = [ArenaClass::Model, ArenaClass::Design];

fn walk_native_markers(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<NativeMarkers, CodecError> {
    let bytes = root.window();
    let mut blocks = Vec::new();
    let mut directory = Vec::new();
    let mut cache_cells = Vec::new();
    let mut given_up = Vec::new();
    let mut unreadable_frames = Vec::new();
    let mut i = outer_hdr::LEN;
    while i + MARKER.len() <= bytes.len() {
        if bytes[i..i + MARKER.len()] != MARKER {
            i += 1;
            continue;
        }
        match ctx.run_part(|| try_block(ctx, root, i, &mut given_up))? {
            Ok(Some(block)) => {
                i = block.offset
                    + block_hdr::LEN
                    + block.preamble_len
                    + index_from_u32(block.comp_sz);
                blocks.push(block.into_block());
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                if let Some((_, _, end)) = read_block_frame(bytes, i) {
                    let cause = cadmpeg_ir::unknown::part::Cause::Unreadable.of(&error);
                    let part = native_block_part(bytes, i, end);
                    given_up.push(ctx.give_up_part(part, cause, error)?);
                    i = end;
                    continue;
                }
                return Err(error);
            }
        }
        if let Some(cell) = try_cache_cell(bytes, i) {
            cache_cells.push(cell);
        } else if let Some(entry) = try_directory_entry(bytes, i) {
            directory.push(entry);
        } else if let Some((_, _, end)) = read_block_frame(bytes, i).filter(|(frame, start, _)| {
            index_from_u32(frame.uncomp_sz) <= MAX_UNCOMP
                && bytes
                    .get(i + block_hdr::LEN..*start)
                    .and_then(nibble_swap_name)
                    .is_some()
        }) {
            unreadable_frames.push((i, end));
        }
        i += 1;
    }
    let tail_start = native_layout::validate(
        bytes,
        &mut blocks,
        &mut unreadable_frames,
        &mut cache_cells,
        &mut directory,
    )?;
    let mut given_up_end = 0;
    for (offset, end) in unreadable_frames {
        if offset >= tail_start || offset < given_up_end {
            continue;
        }
        given_up_end = end;
        given_up.push(ctx.give_up_part(
            native_block_part(bytes, offset, end),
            Cause::Unreadable,
            CodecError::malformed(
                "declared native local member failed DEFLATE, size or CRC validation",
            ),
        )?);
    }
    ctx.charge_entities(u64_from_index(blocks.len()), "admit SLDPRT block")?;
    ctx.charge_entities(u64_from_index(cache_cells.len()), "admit SLDPRT cache cell")?;
    ctx.charge_entities(
        u64_from_index(directory.len()),
        "admit SLDPRT directory entry",
    )?;
    Ok(NativeMarkers {
        blocks,
        directory,
        cache_cells,
        given_up,
    })
}

/// The native block whose frame starts at `offset` and whose payload ends at
/// `end`, as a part to give up.
fn native_block_part(bytes: &[u8], offset: usize, end: usize) -> Part<'_> {
    let part = Part::at(
        "sldprt",
        NATIVE_BLOCK_PART,
        u64_from_index(offset),
        bytes.get(offset..end).unwrap_or_default(),
        SECTION_HOLDS,
    )
    .digest_only();
    match read_block_frame(bytes, offset)
        .and_then(|(_, start, _)| nibble_swap_name(bytes.get(offset + block_hdr::LEN..start)?))
    {
        Some(name) => part.named(name),
        None => part,
    }
}

/// The Parasolid streams `payload`, the content of the stream `stream`,
/// holds for its owner `section`. Where they cannot be extracted, `part`, the
/// source bytes that hold them, is given up and the owner holds none. Each
/// wrapped member that does not inflate is given up as its own part.
fn extract_streams_for_owner(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    stream: &cadmpeg_ir::StreamName,
    section: Option<&str>,
    part: Part<'_>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<crate::parasolid::ExtractedStreams, CodecError> {
    match ctx.run_part(|| crate::parasolid::extract_streams_in_section(payload, section, ctx))? {
        Ok(streams) => {
            for span in &streams.unread {
                let mut member = Part::at(
                    "sldprt",
                    WRAPPED_MEMBER_PART,
                    u64_from_index(span.start),
                    &payload[span.clone()],
                    SECTION_HOLDS,
                )
                .in_stream(stream.clone())
                .digest_only();
                if let Some(section) = section {
                    member = member.named(section);
                }
                given_up.push(ctx.give_up_part(
                    member,
                    Cause::Unreadable,
                    CodecError::malformed(
                        "the ZLB-wrapped frames from this offset do not read: a zlib member \
                         does not inflate to its declared size or passes the frame ceiling, a \
                         frame runs past its section, or bytes after the frames are neither a \
                         frame nor zero padding",
                    ),
                )?);
            }
            Ok(streams)
        }
        Err(error) => {
            let cause = cadmpeg_ir::unknown::part::Cause::Unreadable.of(&error);
            given_up.push(ctx.give_up_part(part, cause, error)?);
            Ok(crate::parasolid::ExtractedStreams::of(Vec::new(), false))
        }
    }
}

fn compound_stream(
    ctx: &DecodeContext<'_>,
    path: &str,
    directory_id: u32,
    start_sector: u32,
    bytes: Vec<u8>,
    decoded_bytes: Option<Vec<u8>>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<CompoundStream, CodecError> {
    let semantic_payload = decoded_bytes.as_deref().unwrap_or(&bytes);
    let stream = compound_stream_name(path, directory_id);
    let part = Part::at("sldprt", PARASOLID_STREAMS_PART, 0, &bytes, SECTION_HOLDS)
        .named(path)
        .in_stream(stream.clone())
        .digest_only();
    let extracted =
        extract_streams_for_owner(ctx, semantic_payload, &stream, Some(path), part, given_up)?;
    Ok(CompoundStream {
        path: stream,
        directory_id,
        start_sector,
        payload: bytes,
        decoded_payload: decoded_bytes,
        ps_streams: extracted.streams,
        ps_probe_capped: extracted.probe_capped,
    })
}

/// The stream name of the CFB stream at `path`, or its directory id where
/// the path is no stream name.
fn compound_stream_name(path: &str, directory_id: u32) -> cadmpeg_ir::StreamName {
    cadmpeg_ir::StreamName::try_from(path.to_owned())
        .unwrap_or_else(|_| cadmpeg_ir::stream_name!("compound@").with_suffix(directory_id))
}

/// Scans an in-memory image, inflating through the decode context's
/// decompressed-byte limit.
pub(crate) fn scan<'a>(
    ctx: &DecodeContext<'_>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    if root.window().starts_with(&COMPOUND_FILE_MAGIC) {
        let compound = compound_streams(ctx, root)?;
        return completed_scan(ctx, root.window(), 0, NativeMarkers::default(), compound);
    }
    let bytes = root.window();
    let mut envelope = root;
    let mut header = envelope
        .req_take_child(outer_hdr::LEN)
        .map_err(|error| error.during("read SLDPRT native outer header"))?;
    // discarded-value: the file ID precedes the required version field
    let _ = header.req_take(outer_hdr::VERSION)?;
    let version = header.req_u32_be()?;
    let native = walk_native_markers(ctx, root)?;
    completed_scan(ctx, bytes, version, native, CompoundStreams::default())
}

/// The streams of a CFB envelope, the streams given up, and the
/// noncanonical encodings read past.
#[derive(Default)]
struct CompoundStreams {
    streams: Vec<CompoundStream>,
    given_up: Vec<GivenUpPart>,
    noncanonical: Vec<String>,
}

/// CFB directory/FAT/open is [`CompoundSnapshot`]; ZLB unwrap and Parasolid
/// extract stay codec-local because they are `SolidWorks` payload semantics, not
/// CFB.
fn compound_streams(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<CompoundStreams, CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    let mut compound = CompoundStreams::default();
    for deviation in snapshot.deviations() {
        let stream = deviation
            .stream()
            .and_then(|id| snapshot.stream_by_id(id))
            .map(cadmpeg_container::compound::CompoundStreamEntry::path);
        match (deviation, stream) {
            (CompoundDeviation::StreamChainBroken(_), _) => {}
            (_, Some(path)) => compound
                .noncanonical
                .push(format!("{path}: {}", deviation.description())),
            (_, None) => compound
                .noncanonical
                .push(deviation.description().to_owned()),
        }
    }
    for entry in snapshot.entries() {
        let CompoundEntry::Stream(entry) = entry else {
            continue;
        };
        let opened = ctx.run_part(|| {
            let view = snapshot.open(ctx, entry)?;
            ctx.retain_bytes(
                u64_from_index(view.window().len()),
                "retain SolidWorks CFB stream",
            )?;
            Ok((view.window().to_vec(), decode_wrapped_payload(ctx, view)?))
        })?;
        let path = entry.path();
        let directory_id = entry.id().directory_id();
        let (payload, decoded) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                let located = match snapshot.open_located(ctx, entry) {
                    Ok(Some(located)) => Some(located),
                    Ok(None) | Err(_) => snapshot.open(ctx, entry).ok(),
                };
                let bytes = located.as_ref().map_or(&[][..], |view| view.window());
                let declared = entry.logical_size();
                let mut part = Part::at("sldprt", COMPOUND_STREAM_PART, 0, bytes, SECTION_HOLDS)
                    .named(path)
                    .in_stream(compound_stream_name(path, directory_id))
                    .digest_only();
                if u64_from_index(bytes.len()) != declared {
                    part = part.costing(format!(
                        "the stream declares {declared} bytes; its length counts the {} bytes \
                         its sector chain located",
                        bytes.len()
                    ));
                }
                let cause = cadmpeg_ir::unknown::part::Cause::Unreadable.of(&error);
                compound
                    .given_up
                    .push(ctx.give_up_part(part, cause, error)?);
                continue;
            }
        };
        let stream = compound_stream(
            ctx,
            path,
            directory_id,
            entry.start_sector(),
            payload,
            decoded,
            &mut compound.given_up,
        )?;
        compound.streams.push(stream);
    }
    Ok(compound)
}

fn decode_wrapped_payload(
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
    ps_probe_capped: bool,
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
            ps_probe_capped: self.ps_probe_capped,
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
    given_up: &mut Vec<GivenUpPart>,
) -> Result<Option<RawBlock>, CodecError> {
    if inflated.len() != index_from_u32(frame.uncomp_sz) {
        return Ok(None);
    }
    if crc32fast::hash(&inflated) != frame.crc {
        return Ok(None);
    }

    let payload_start = off + block_hdr::LEN + index_from_u32(frame.pre_sz);
    let preamble = bytes
        .get(off + block_hdr::LEN..payload_start)
        .unwrap_or(&[]);
    let section = nibble_swap_name(preamble);
    // A Parasolid block is one from which a `PS\0\0` stream can be extracted (in
    // plain, wrapped, or nested form); otherwise fall back to a byte-signature
    // family label.
    let payload_end = payload_start + index_from_u32(frame.comp_sz);
    let mut part = Part::at(
        "sldprt",
        PARASOLID_STREAMS_PART,
        u64_from_index(off),
        bytes.get(off..payload_end).unwrap_or_default(),
        SECTION_HOLDS,
    )
    .digest_only();
    if let Some(name) = &section {
        part = part.named(name.as_str());
    }
    let stream = section
        .as_ref()
        .and_then(|name| cadmpeg_ir::StreamName::try_from(name.clone()).ok())
        .unwrap_or_else(|| cadmpeg_ir::stream_name!("block@").with_suffix(off));
    let extracted =
        extract_streams_for_owner(ctx, &inflated, &stream, section.as_deref(), part, given_up)?;
    let family = if extracted.streams.is_empty() {
        payload_family(&inflated)
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
        ps_streams: extracted.streams,
        ps_probe_capped: extracted.probe_capped,
    }))
}

fn try_block(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    off: usize,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<Option<RawBlock>, CodecError> {
    let bytes = root.window();
    let Some((frame, payload_start, payload_end)) = read_block_frame(bytes, off) else {
        return Ok(None);
    };
    // A marker candidate that declares more than the block ceiling is not a
    // block; the marker walk continues with the next candidate.
    if index_from_u32(frame.uncomp_sz) > MAX_UNCOMP {
        return Ok(None);
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
    block_from_inflated(ctx, bytes, off, &frame, inflated, given_up)
}

fn try_cache_cell(bytes: &[u8], off: usize) -> Option<CacheCell> {
    let (two_l, half_l, l, name_len) = (|| {
        Some((
            View::u32_le_at(bytes, off + cache_hdr::TWO_L)?,
            View::u32_le_at(bytes, off + cache_hdr::HALF_L)?,
            View::u32_le_at(bytes, off + cache_hdr::L)?,
            View::u32_le_at(bytes, off + cache_hdr::NAME_LEN)?,
        ))
    })()?;

    if l == 0 || l.checked_mul(2) != Some(two_l) || half_l != l / 2 {
        return None;
    }
    if name_len == 0 || name_len >= 500 {
        return None;
    }
    let name_start = off + cache_hdr::LEN;
    let raw = bytes.get(name_start..name_start + index_from_u32(name_len))?;
    let name = nibble_swap_name(raw)?;
    Some(CacheCell {
        offset: off,
        logical_len: l,
        physical_range: None,
        name,
    })
}

fn try_directory_entry(bytes: &[u8], off: usize) -> Option<DirectoryEntry> {
    let (type_id, zero_a, size, zero_b, name_len) = (|| {
        Some((
            View::u32_le_at(bytes, off + dir_ent::TYPE_ID)?,
            View::u32_le_at(bytes, off + dir_ent::ZERO_AT_10)?,
            View::u32_le_at(bytes, off + dir_ent::SIZE)?,
            View::u32_le_at(bytes, off + dir_ent::ZERO_AT_18)?,
            View::u32_le_at(bytes, off + dir_ent::NAME_LEN)?,
        ))
    })()?;
    if zero_a != 0 || zero_b != 0 {
        return None;
    }
    if name_len == 0 || name_len >= 500 {
        return None;
    }
    let name_start = off + dir_ent::LEN;
    let raw = bytes.get(name_start..name_start + index_from_u32(name_len))?;
    let descriptor = bytes
        .get(off + dir_ent::DESCRIPTOR..off + dir_ent::LEN)
        .and_then(|value| value.try_into().ok())?;
    bytes.get(name_start + index_from_u32(name_len)..name_start + index_from_u32(name_len) + 6)?;
    let name = nibble_swap_name(raw)?;
    Some(DirectoryEntry {
        offset: off,
        type_id,
        size,
        name,
        descriptor,
    })
}

/// Convert a scan into the generic container inventory returned by
/// [`cadmpeg_ir::Codec::inspect`].
pub(crate) fn summarize(scan: &ContainerScan, dialects: DialectLayers) -> ContainerSummary {
    let mut entries = Vec::new();

    for b in &scan.blocks {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_owned(), b.offset.to_string());
        attributes.insert("type_id".to_owned(), format!("0x{:08x}", b.type_id));
        attributes.insert("family".to_owned(), b.family.label().to_owned());
        attributes.insert("sha256".to_owned(), sha256_hex(&b.payload));
        if let Some(stream) = b.ps_streams.first() {
            attributes.insert(
                "parasolid_schema".to_owned(),
                stream.header.schema.value().to_string(),
            );
            attributes.insert(
                "parasolid_description".to_owned(),
                stream.header.description.clone(),
            );
        }
        entries.push(ContainerEntry {
            name: b.section.source_stream().as_str().to_owned(),
            role: ContainerRole::Block,
            storage: EntryStorage::Compressed {
                method: CompressionMethod::Deflate,
                stored: Some(u64::from(b.comp_sz)),
                expanded: Some(u64_from_index(b.uncomp_sz())),
            },
            attributes,
        });
    }

    for d in &scan.directory {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_owned(), d.offset.to_string());
        attributes.insert("type_id".to_owned(), format!("0x{:08x}", d.type_id));
        entries.push(ContainerEntry {
            name: d.name.as_str().to_owned(),
            role: ContainerRole::DirectoryEntry,
            storage: EntryStorage::payload_only(VerbatimLabel::None, u64::from(d.size)),
            attributes,
        });
    }

    for c in &scan.cache_cells {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_owned(), c.offset.to_string());
        attributes.insert("logical_len".to_owned(), c.logical_len.to_string());
        entries.push(ContainerEntry {
            name: c.name.as_str().to_owned(),
            role: ContainerRole::CacheCell,
            storage: EntryStorage::payload_only(VerbatimLabel::None, u64::from(c.logical_len)),
            attributes,
        });
    }

    for stream in &scan.compound_streams {
        let mut attributes = BTreeMap::new();
        attributes.insert("start_sector".to_owned(), stream.start_sector.to_string());
        attributes.insert("sha256".to_owned(), sha256_hex(&stream.payload));
        attributes.insert(
            "family".to_owned(),
            payload_family(&stream.payload).label().to_owned(),
        );
        entries.push(ContainerEntry {
            name: stream.path.as_str().to_owned(),
            role: ContainerRole::CompoundStream,
            storage: EntryStorage::verbatim(
                VerbatimLabel::Stored,
                u64_from_index(stream.payload.len()),
            ),
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

const NO_ACTIVE_PARASOLID_NOTE: &str =
    "no unique active Parasolid partition located; available B-rep sites remain decodable";

pub(crate) fn notes(scan: &ContainerScan<'_>) -> Vec<String> {
    let mut notes = Vec::new();
    let given_up_streams = scan
        .given_up
        .iter()
        .filter(|part| part.kind() == COMPOUND_STREAM_PART)
        .count();
    let given_up_streams = if given_up_streams == 0 {
        String::new()
    } else {
        format!(" read and {given_up_streams} given up")
    };
    notes.push(format!(
        "outer version word: 0x{:08x}; {} CRC-validated block(s), {} tail-directory \
             entry/entries, {} cache-cell(s), {} compound stream(s){given_up_streams}",
        scan.version,
        scan.blocks.len(),
        scan.directory.len(),
        scan.cache_cells.len(),
        scan.compound_streams.len()
    ));
    notes.push(match active_parasolid_summary(scan) {
        Some((name, size, schema)) => format!(
            "active Parasolid B-rep candidate: {} ({} bytes, schema {})",
            name,
            size,
            schema.schema.value()
        ),
        None => NO_ACTIVE_PARASOLID_NOTE.to_owned(),
    });
    notes.push(
        "Parasolid body streams supply the typed topology and analytic carriers used by decode"
            .to_owned(),
    );
    notes
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
#[derive(Clone, Copy)]
pub(crate) struct ActiveParasolidSite<'a> {
    pub(crate) section: Section<'a>,
    /// Offset of the stream in its section's semantic payload.
    pub(crate) offset: usize,
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

pub(crate) fn is_stock_bodies_section(name: &str) -> bool {
    let name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name.strip_prefix("config-")
        .and_then(|name| name.strip_suffix("-stockbodies"))
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
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
    if active_configuration.is_none()
        && (has_manifest_configuration_rows(scan)
            || !unread_active_partition_sections(scan).is_empty())
    {
        return None;
    }
    let mut selected = None;
    for section in scan.sections() {
        let name = section.name().unwrap_or("");
        let section_is_partition = contains_ascii_case_insensitive(name, "partition")
            && !contains_ascii_case_insensitive(name, "ghost")
            && !contains_ascii_case_insensitive(name, "deltas")
            && !contains_ascii_case_insensitive(name, "resolvedfeatures");
        let section_is_admissible = !contains_ascii_case_insensitive(name, "ghost")
            && !contains_ascii_case_insensitive(name, "deltas")
            && !contains_ascii_case_insensitive(name, "resolvedfeatures")
            && !is_stock_bodies_section(name)
            && virtual_component_key(name).is_none();
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
                offset: stream.offset,
                header: &stream.header,
            });
        }
    }
    selected
}

/// Whether `name` names a section that can hold a configuration's Parasolid
/// partition.
pub(crate) fn is_partition_section(name: &str) -> bool {
    contains_ascii_case_insensitive(name, "partition")
        && !contains_ascii_case_insensitive(name, "ghost")
        && !contains_ascii_case_insensitive(name, "deltas")
        && !contains_ascii_case_insensitive(name, "resolvedfeatures")
        && !is_stock_bodies_section(name)
        && virtual_component_key(name).is_none()
}

/// The given-up sections that the choice of the active partition needs:
/// each partition section, and each features manifest where no manifest row
/// was read.
pub(crate) fn unread_active_partition_sections<'s>(scan: &'s ContainerScan<'_>) -> Vec<&'s str> {
    let manifest_read = has_manifest_configuration_rows(scan);
    scan.given_up
        .iter()
        .filter(|part| {
            matches!(
                part.kind(),
                COMPOUND_STREAM_PART
                    | NATIVE_BLOCK_PART
                    | PARASOLID_STREAMS_PART
                    | WRAPPED_MEMBER_PART
            )
        })
        .filter_map(GivenUpPart::name)
        .filter(|name| {
            is_partition_section(name) || (!manifest_read && is_features_manifest_name(Some(name)))
        })
        .collect()
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
    if has_manifest_configuration_rows(scan) {
        return scan
            .solidworks
            .manifest_active_configuration
            .unique_ref()
            .map(|(index, _)| index);
    }
    explicit_active_configuration_index(scan)
}

pub(crate) fn has_manifest_configuration_rows(scan: &ContainerScan) -> bool {
    scan.solidworks.configuration_bindings.has_manifest_rows()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestConfigurationIndex {
    Absent,
    Unresolved,
    Resolved(usize),
}

pub(crate) fn manifest_configuration_index(
    scan: &ContainerScan,
    name: &str,
) -> ManifestConfigurationIndex {
    scan.solidworks.configuration_bindings.index_for(name)
}

pub(crate) fn manifest_configuration_name<'a>(
    scan: &'a ContainerScan<'_>,
    index: usize,
) -> Option<&'a str> {
    if !has_manifest_configuration_rows(scan) {
        return None;
    }
    scan.solidworks.configuration_bindings.name_for(index)
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
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("Features") || name.eq_ignore_ascii_case("Features__ZLB")
        })
}

fn explicit_active_configuration_index(scan: &ContainerScan<'_>) -> Option<usize> {
    let active = active_configuration_name_ref(scan)?;
    let indices = scan.solidworks.configuration_source_indices.get(active)?;
    (indices.len() == 1).then(|| indices[0])
}

pub(crate) fn active_configuration_name_ref<'a>(scan: &'a ContainerScan<'_>) -> Option<&'a str> {
    scan.solidworks
        .manifest_active_configuration
        .unique_ref()
        .and_then(|(_, name)| name)
        .or_else(|| {
            (matches!(
                scan.solidworks.manifest_active_configuration,
                ManifestActiveConfiguration::Absent
            ) && scan.solidworks.configuration_names.len() == 1)
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

/// Section folder of embedded component documents.
const EMBEDDED_PREFIX: &str = "ImportedComp/";
/// Extensions that end the section name of an embedded document.
const EMBEDDED_EXTENSIONS: [&str; 2] = [".sldprt", ".sldasm"];

/// Whether a section is a complete embedded document (`sldprt.md` §11.4).
/// The name suffix of a compound stream in the nested payload wrapper (spec §1).
const WRAPPED_NAME_SUFFIX: &str = "__ZLB";

/// A section name without one trailing wrapper suffix.
pub(crate) fn unwrapped_name(name: &str) -> &str {
    name.len()
        .checked_sub(WRAPPED_NAME_SUFFIX.len())
        .filter(|&at| {
            name.get(at..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(WRAPPED_NAME_SUFFIX))
        })
        .and_then(|at| name.get(..at))
        .unwrap_or(name)
}

pub(crate) fn is_embedded_document(name: &str) -> bool {
    let name = unwrapped_name(name);
    let ends_with = |suffix: &str| {
        name.len()
            .checked_sub(suffix.len())
            .and_then(|start| name.get(start..))
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
    };
    name.starts_with(EMBEDDED_PREFIX)
        || EMBEDDED_EXTENSIONS
            .iter()
            .any(|extension| ends_with(extension))
}

/// Folder of the sections that hold virtual component documents (`sldprt.md`
/// §11.5).
const VIRTUAL_PREFIX: &str = "VirtualComp/";
/// Suffix that ends the section name of a virtual component document.
const VIRTUAL_SUFFIX: &str = "-sldprt";

/// The key of a virtual component section, its name after `VirtualComp/` in
/// ASCII lower case, or `None` for another section (`sldprt.md` §11.5).
pub(crate) fn virtual_component_key(name: &str) -> Option<String> {
    let name = unwrapped_name(name).replace('\\', "/");
    let prefix = name.get(..VIRTUAL_PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(VIRTUAL_PREFIX) {
        return None;
    }
    let key = name.get(VIRTUAL_PREFIX.len()..)?.to_ascii_lowercase();
    (key.len() > VIRTUAL_SUFFIX.len() && key.ends_with(VIRTUAL_SUFFIX)).then_some(key)
}

/// Lead bytes that a section stores before its XML text.
const XML_LEAD_MARKERS: [u8; 2] = [0x85, 0x86];

pub(crate) fn xml_text(bytes: &[u8]) -> Option<String> {
    let bytes = match bytes.split_first() {
        Some((lead, rest)) if XML_LEAD_MARKERS.contains(lead) => rest,
        _ => bytes,
    };
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

/// `text` stored in the encoding of `original`, a section that
/// [`xml_text`] reads: its lead byte, then UTF-16LE after a byte-order mark
/// or UTF-8.
pub(crate) fn xml_bytes(original: &[u8], text: &str) -> Vec<u8> {
    let (mut output, body) = match original.split_first() {
        Some((lead, rest)) if XML_LEAD_MARKERS.contains(lead) => (vec![*lead], rest),
        _ => (Vec::new(), original),
    };
    if body.starts_with(&[0xff, 0xfe]) {
        output.extend_from_slice(&[0xff, 0xfe]);
        output.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    } else {
        output.extend_from_slice(text.as_bytes());
    }
    output
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
    configuration_bindings: configuration_bindings::ConfigurationBindings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum ManifestActiveConfiguration {
    #[default]
    Absent,
    Unique(usize, Option<String>),
    Ambiguous,
}

impl ManifestActiveConfiguration {
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

/// Scans the `swSolidWorks` envelopes of `sections`, each parsed in the
/// session `ctx` under `operation`. A resource refusal ends the scan; a
/// section that does not parse holds no envelope.
fn scan_solidworks_envelopes<'a>(
    sections: impl IntoIterator<Item = (Option<&'a str>, &'a [u8])>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<SolidWorksEnvelopeScan, CodecError> {
    let mut scan = SolidWorksEnvelopeScan::default();
    for (section, payload) in sections {
        let Some(text) = xml_text(payload) else {
            continue;
        };
        let document = match ctx.parse_xml(&text, operation) {
            Ok(tree) => tree,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => continue,
        };
        let root = document.root_element();
        if is_features_manifest_name(section) && root.tag_name().name() == "swSolidWorks" {
            scan.configuration_bindings.record_features(document.tree());
        }
        if root.tag_name().name().contains("Keywords") {
            scan.configuration_bindings.record_keywords(document.tree());
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
                    indices.push(index);
                } else {
                    scan.configuration_source_indices
                        .insert(name.to_owned(), vec![index]);
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
                scan.configuration_names.insert(name.to_owned());
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
                    source_attributes.insert((slot, target), value);
                }
            }
        }
        let mut configuration_attributes = BTreeMap::new();
        for ((slot, target), value) in source_attributes {
            configuration_attributes.insert(
                format!("sw_configuration_{slot}_{target}"),
                value.to_owned(),
            );
        }
        scan.first = Some(SolidWorksEnvelope {
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
    scan.configuration_bindings.finish();
    scan.manifest_active_configuration = scan.configuration_bindings.active();
    Ok(scan)
}

/// Returns the first parsed `swSolidWorks` envelope of `payloads` even if
/// an attribute is absent, each payload parsed in the session `ctx`.
///
/// # Errors
///
/// Returns [`CodecError::ResourceLimit`] where a parse meets a ceiling of
/// `ctx`.
pub(crate) fn first_solidworks_envelope<'a>(
    ctx: &DecodeContext<'_>,
    payloads: impl IntoIterator<Item = &'a [u8]>,
) -> Result<Option<SolidWorksEnvelope>, CodecError> {
    Ok(scan_solidworks_envelopes(
        payloads.into_iter().map(|payload| (None, payload)),
        ctx,
        "SLDPRT retained envelope XML tree",
    )?
    .first)
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
