// SPDX-License-Identifier: Apache-2.0
//! Outer `.sldprt` container scanning and inspection.
//!
//! Files start with an 8-byte `file_id` and big-endian version header. A shared
//! marker introduces raw-DEFLATE blocks, cache cells, and tail-directory
//! entries. [`scan`] classifies marker occurrences with structure-specific
//! invariants, validates block CRC-32 values, inflates payloads, decodes stored
//! section names, and extracts embedded Parasolid streams.

use std::collections::BTreeMap;
use std::io::Read;

use cadmpeg_ir::codec::{ContainerEntry, ContainerSummary};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::wire::hash::sha256_hex;
use cadmpeg_ir::wire::le::u32_at as u32_le;

/// Marker shared by block, cache-cell, and directory frames.
pub const MARKER: [u8; 6] = [0x14, 0x00, 0x06, 0x00, 0x08, 0x00];

/// Bytes between a marker and its preamble in a block frame
/// (`marker[6] + type_id[4] + crc32[4] + comp_sz[4] + uncomp_sz[4] + pre_sz[4]`).
const BLOCK_HEADER_LEN: usize = 26;

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
    let Some(header_size) = u32_le(payload, 4) else {
        return false;
    };
    let Some(bits_per_pixel) = payload
        .get(18..20)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    else {
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
pub struct ContainerScan {
    /// Complete source image for exact passthrough writing.
    pub source_image: Vec<u8>,
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

impl ContainerScan {
    pub(crate) fn sections(&self) -> impl Iterator<Item = Section<'_>> {
        self.blocks
            .iter()
            .map(Section::Block)
            .chain(self.compound_streams.iter().map(Section::Compound))
    }
}

/// The outer header magic length (`file_id` + `version`).
const OUTER_HEADER_LEN: usize = 8;
const COMPOUND_FILE_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// Test whether a prefix contains the container marker after its outer header.
///
/// This structural check does not validate block framing or CRC-32.
pub fn looks_like_sldprt(prefix: &[u8]) -> bool {
    if prefix.starts_with(&COMPOUND_FILE_MAGIC)
        && contains_utf16le_ascii(prefix, b"ISolidWorksInformation")
    {
        return true;
    }
    if prefix.len() < OUTER_HEADER_LEN + MARKER.len() {
        return false;
    }
    prefix[OUTER_HEADER_LEN..]
        .windows(MARKER.len())
        .any(|w| w == MARKER)
}

fn contains_utf16le_ascii(haystack: &[u8], text: &[u8]) -> bool {
    let mut encoded = Vec::with_capacity(text.len() * 2);
    for byte in text {
        encoded.extend_from_slice(&[*byte, 0]);
    }
    contains(haystack, &encoded)
}

/// Scan a `.sldprt` decode root into its container structure.
///
/// `_ctx` is taken for parity with the other container codecs' `scan(ctx, root)`
/// entry points; the block scan is a pure function of the source bytes gated by
/// per-block CRC validation, so it charges no decode budget.
pub fn scan(_ctx: &DecodeContext<'_>, root: View<'_>) -> ContainerScan {
    scan_image(root.window())
}

/// Scan an in-memory `.sldprt` image.
///
/// The byte-slice entry to the same core as [`scan`], for the writer paths that
/// scan a retained source image rather than a decode root.
///
/// Truncated input produces a scan containing every structure that could be
/// validated; missing outer-header bytes yield version zero.
pub fn scan_bytes(bytes: &[u8]) -> ContainerScan {
    scan_image(bytes)
}

fn scan_image(bytes: &[u8]) -> ContainerScan {
    if bytes.starts_with(&COMPOUND_FILE_MAGIC) {
        let compound_streams = crate::compound::streams(bytes)
            .unwrap_or_default()
            .into_iter()
            .map(|stream| {
                let located_streams = crate::parasolid::extract_streams_with_offsets(&stream.bytes);
                let ps_stream_offsets = located_streams.iter().map(|(offset, _)| *offset).collect();
                let ps_streams = located_streams
                    .into_iter()
                    .map(|(_, payload)| payload)
                    .collect();
                CompoundStream {
                    path: stream.path,
                    directory_id: stream.directory_id,
                    start_sector: stream.start_sector,
                    payload: stream.bytes,
                    decoded_payload: stream.decoded_bytes,
                    ps_streams,
                    ps_stream_offsets,
                }
            })
            .collect();
        return ContainerScan {
            source_image: bytes.to_vec(),
            version: 0,
            blocks: Vec::new(),
            directory: Vec::new(),
            cache_cells: Vec::new(),
            compound_streams,
        };
    }
    let version = cadmpeg_ir::wire::be::u32_at(bytes, 4).unwrap_or(0);

    let mut blocks = Vec::new();
    let mut directory = Vec::new();
    let mut cache_cells = Vec::new();

    let mut i = OUTER_HEADER_LEN;
    // Every marker hit is tried as a block first (the CRC gate is effectively
    // false-positive-free), then as a cache cell, then as a directory entry.
    while i + MARKER.len() <= bytes.len() {
        if bytes[i..i + MARKER.len()] != MARKER {
            i += 1;
            continue;
        }
        if let Some(block) = try_block(bytes, i) {
            i = block.offset + BLOCK_HEADER_LEN + block.preamble_len + block.comp_sz as usize;
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
        source_image: bytes.to_vec(),
        version,
        blocks,
        directory,
        cache_cells,
        compound_streams: Vec::new(),
    }
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
    let type_id = u32_le(bytes, off + 6)?;
    let crc = u32_le(bytes, off + 10)?;
    let comp_sz = u32_le(bytes, off + 14)?;
    let uncomp_sz = u32_le(bytes, off + 18)?;
    let pre_sz = u32_le(bytes, off + 22)?;

    let comp = comp_sz as usize;
    let pre = pre_sz as usize;
    let uncomp = uncomp_sz as usize;
    if comp == 0 || uncomp == 0 || uncomp > MAX_UNCOMP {
        return None;
    }
    let payload_start = off + BLOCK_HEADER_LEN + pre;
    let payload = bytes.get(payload_start..payload_start + comp)?;

    let inflated = raw_inflate(payload, uncomp)?;
    if inflated.len() != uncomp {
        return None;
    }
    if crc32(&inflated) != crc {
        return None;
    }

    let preamble = bytes
        .get(off + BLOCK_HEADER_LEN..payload_start)
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
    let mut out = Vec::with_capacity(hint.min(1 << 20));
    let mut dec = DeflateDecoder::new(data);
    match dec.read_to_end(&mut out) {
        Ok(_) => Some(out),
        Err(_) => None,
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
    let two_l = u32_le(bytes, off + 10)?;
    let half_l = u32_le(bytes, off + 14)?;
    let l = u32_le(bytes, off + 18)?;
    let name_len = u32_le(bytes, off + 22)?;

    if l == 0 || two_l != l.wrapping_mul(2) || half_l != l / 2 {
        return None;
    }
    if name_len == 0 || name_len >= 500 {
        return None;
    }
    let name_start = off + 26;
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
    let type_id = u32_le(bytes, off + 6)?;
    let zero_a = u32_le(bytes, off + 10)?;
    let size = u32_le(bytes, off + 14)?;
    let zero_b = u32_le(bytes, off + 18)?;
    let name_len = u32_le(bytes, off + 22)?;
    if zero_a != 0 || zero_b != 0 {
        return None;
    }
    if name_len == 0 || name_len >= 500 {
        return None;
    }
    let name_start = off + 40; // 26 + 14-byte descriptor
    let raw = bytes.get(name_start..name_start + name_len as usize)?;
    let name = nibble_swap_name(raw)?;
    Some(DirectoryEntry {
        offset: off,
        type_id,
        size,
        name,
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
            "no Parasolid partition/deltas stream located; B-rep decode will be container-only"
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

fn active_parasolid_summary(
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
    scan.compound_streams
        .iter()
        .flat_map(|stream| {
            stream.ps_streams.iter().filter_map(move |payload| {
                let header = crate::parasolid::stream_header(payload)?;
                crate::parasolid::is_body_stream(&header).then_some((
                    stream.path.clone(),
                    payload.len(),
                    header,
                ))
            })
        })
        .max_by_key(|(_, size, _)| *size)
}

/// Modeller generation carried by the active Parasolid stream schema.
pub(crate) fn active_parasolid_modeler_generation(scan: &ContainerScan) -> Option<u32> {
    let (_, _, header) = active_parasolid_summary(scan)?;
    parasolid_modeler_generation(&header.schema)
}

pub(crate) fn parasolid_modeler_generation(schema: &str) -> Option<u32> {
    let body = schema.strip_prefix("SCH_")?;
    body.strip_prefix("SW_")
        .unwrap_or(body)
        .split('_')
        .next()?
        .get(..2)?
        .parse()
        .ok()
}

/// Test whether either outer envelope carries a framed Parasolid body stream.
pub fn has_parasolid_body_stream(scan: &ContainerScan) -> bool {
    active_parasolid_summary(scan).is_some()
}

/// Select the highest-ranked Parasolid B-rep block.
///
/// Ranking favors larger partition streams, then deltas streams. Ghost and
/// `ResolvedFeatures` sections receive a penalty. The return value includes the
/// parsed stream header.
pub fn select_active_parasolid(
    scan: &ContainerScan,
) -> Option<(&Block, crate::parasolid::StreamHeader)> {
    let active_configuration = active_configuration_index(scan);
    let mut best: Option<(i64, &Block, crate::parasolid::StreamHeader)> = None;
    for b in &scan.blocks {
        let Some(ps) = &b.ps_stream else { continue };
        let Some(sch) = crate::parasolid::stream_header(ps) else {
            continue;
        };
        let name = b.section.as_deref().unwrap_or("").to_ascii_lowercase();
        let desc = sch.description.to_ascii_lowercase();

        // Larger real streams score higher; the ghost stub and feature lane are
        // demoted below any genuine partition/deltas body.
        let mut score = (ps.len() / 64) as i64;
        if name.contains("ghost") || desc.contains("ghost") {
            score -= 1_000_000;
        }
        if name.contains("resolvedfeatures") {
            score -= 1_000_000;
        }
        if name.contains("partition") {
            score += 100_000;
            if active_configuration.is_some_and(|index| configuration_index(&name) == Some(index)) {
                score += 1_000_000;
            }
        } else if name.contains("deltas") || desc.contains("deltas") {
            score += 50_000;
        }

        if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
            best = Some((score, b, sch));
        }
    }
    best.map(|(_, b, sch)| (b, sch))
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
    let active = scan.blocks.iter().find_map(|block| {
        let text = std::str::from_utf8(&block.payload).ok()?;
        let document = roxmltree::Document::parse(text).ok()?;
        let root = document.root_element();
        (root.tag_name().name() == "swSolidWorks")
            .then(|| {
                root.descendants()
                    .find(|node| node.has_tag_name("swModel"))?
                    .attribute("swConfigurationName")
            })
            .flatten()
            .map(str::to_string)
    })?;
    let (position, explicit_index, configuration_count) = scan.blocks.iter().find_map(|block| {
        let text = std::str::from_utf8(&block.payload).ok()?;
        let document = roxmltree::Document::parse(text).ok()?;
        let root = document.root_element();
        root.tag_name().name().contains("Keywords").then_some(())?;
        let configurations = root
            .children()
            .filter(|node| node.has_tag_name("Configuration"))
            .collect::<Vec<_>>();
        let position = configurations
            .iter()
            .position(|node| node.attribute("Name") == Some(active.as_str()))?;
        let explicit_index = configurations[position]
            .attribute("SourceIndex")
            .and_then(|value| value.parse().ok());
        Some((position, explicit_index, configurations.len()))
    })?;
    if explicit_index.is_some() {
        return explicit_index;
    }
    let mut partitions = scan
        .blocks
        .iter()
        .filter(|block| {
            block
                .section
                .as_deref()
                .is_some_and(|section| section.to_ascii_lowercase().ends_with("-partition"))
        })
        .filter_map(|block| configuration_index(block.section.as_deref()?))
        .collect::<Vec<_>>();
    partitions.sort_unstable();
    partitions.dedup();
    if partitions.len() == configuration_count {
        return partitions.get(position).copied();
    }
    partitions.contains(&position).then_some(position)
}

#[cfg(test)]
mod tests {
    use super::parasolid_modeler_generation;

    #[test]
    fn parasolid_schema_starts_with_the_modeller_generation() {
        assert_eq!(
            parasolid_modeler_generation("SCH_3000310_30000_13006"),
            Some(30)
        );
        assert_eq!(
            parasolid_modeler_generation("SCH_3101284_31100_13006"),
            Some(31)
        );
        assert_eq!(parasolid_modeler_generation("SCH_SW_33103_11000"), Some(33));
    }
}
