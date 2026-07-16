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

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use cadmpeg_ir::decode::{ByteRange, DecodeContext, DerivedKind, ExpandSpec, TransformKind, View};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::le::u32_at as u32_le;

/// Marker shared by block, cache-cell, and directory frames.
pub const MARKER: [u8; 6] = [0x14, 0x00, 0x06, 0x00, 0x08, 0x00];

/// Read chunk for streaming a block's inflated DEFLATE output into the platform
/// expander. A fixed stack buffer, so no decompressed bytes are retained outside
/// the [`cadmpeg_ir::decode::ExpandWriter`] before finalize (§10 Phase 1B gate 2).
const EXPAND_CHUNK: usize = 16 * 1024;

/// Allocation charged per validated block admitted into the runtime space graph
/// (§10 Phase 1A).
///
/// Covers the fixed heap footprint each block adds: the space-graph record for
/// its decompressed `Transform` space and the [`Block`] summary row. This rounds
/// up rather than measuring the platform-internal layouts, which the codec cannot
/// see; its purpose is to make the block count consume the input-proportional
/// allocation budget so a file packed with minimal frames cannot grow the graph
/// without a matching charge.
const PER_BLOCK_GRAPH_BYTES: u64 = 256;

/// Bytes between a marker and its preamble in a block frame
/// (`marker[6] + type_id[4] + crc32[4] + comp_sz[4] + uncomp_sz[4] + pre_sz[4]`).
const BLOCK_HEADER_LEN: usize = 26;

/// Upper bound on a single decompressed block, guarding a corrupt `uncomp_sz`
/// from driving an unbounded allocation. Real part streams sit far below this.
///
/// Limit classification (§10 Phase 1 table): deployment ceiling. On the session
/// decode path `read_root` enforces the platform `max_input_bytes` policy limit
/// first and [`DecodeContext::begin_expand`] bounds the decompressed output
/// against the per-expand and cumulative decompression envelope; this tighter
/// codec-local cap is retained as dual enforcement — and remains the sole bound
/// on the writer-side [`scan_bytes`] path, which does not run under a session —
/// until per-profile calibration justifies migrating it wholesale.
const MAX_UNCOMP: usize = 512 * 1024 * 1024;

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// A CRC-validated compressed block (payload family in `attributes`).
    pub const BLOCK: &str = "block";
    /// A tail section-directory entry naming one OPC part.
    pub const DIRECTORY_ENTRY: &str = "directory-entry";
    /// A cache-cell section-index grid entry (not a compressed payload).
    pub const CACHE_CELL: &str = "cache-cell";
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
}

/// The outer header magic length (`file_id` + `version`).
const OUTER_HEADER_LEN: usize = 8;

/// Test whether a prefix contains the container marker after its outer header.
///
/// This structural check does not validate block framing or CRC-32.
pub fn looks_like_sldprt(prefix: &[u8]) -> bool {
    if prefix.len() < OUTER_HEADER_LEN + MARKER.len() {
        return false;
    }
    prefix[OUTER_HEADER_LEN..]
        .windows(MARKER.len())
        .any(|w| w == MARKER)
}

/// Read and scan a complete `.sldprt` stream.
///
/// Block candidates must inflate to their declared size and match their stored
/// CRC-32. Cache cells and directory entries must satisfy their framing
/// invariants. Unclassified marker occurrences are ignored.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(CodecError::Io)?;
    Ok(scan_bytes(&bytes))
}

/// Consume the session root view directly (§10 Phase 1A) and scan the container.
///
/// The marker walk visits every input byte once; its bytes are charged as work
/// before scanning begins. Every block's DEFLATE payload is inflated through the
/// platform expander ([`DecodeContext::begin_expand`], §10 Phase 1B), and each
/// decompressed block plus its nested Parasolid streams is registered in the
/// runtime space graph ("L1-ready", not emitting L1 — the v2 ledger schema is not
/// yet serialized). Both `inspect` and `decode` enter here, so they run one
/// shared container policy (graduation-gate item 6).
///
/// The returned scan owns its block payloads: the retained source image and the
/// preserved per-block records are IR-level requirements, so the validated
/// decompressed bytes are copied out of the arena once, at admission.
pub fn scan_view(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<ContainerScan, CodecError> {
    let bytes = root.window();
    ctx.charge_work(
        bytes.len() as u64,
        "sldprt_container_scan",
        Some(root.location()),
    )?;

    let version = cadmpeg_ir::be::u32_at(bytes, 4).unwrap_or(0);
    let mut blocks = Vec::new();
    let mut directory = Vec::new();
    let mut cache_cells = Vec::new();

    let mut i = OUTER_HEADER_LEN;
    while i + MARKER.len() <= bytes.len() {
        if bytes[i..i + MARKER.len()] != MARKER {
            i += 1;
            continue;
        }
        if let Some(raw) = admit_block(ctx, root, bytes, i)? {
            // Each admitted block pushes a space-graph record and a summary row;
            // bound that per-block footprint against the input-proportional
            // allocation budget as blocks are admitted.
            ctx.charge_alloc(
                PER_BLOCK_GRAPH_BYTES,
                "sldprt_container_block",
                Some(root.location()),
            )?;
            i = raw.offset + BLOCK_HEADER_LEN + raw.preamble_len + raw.comp_sz as usize;
            blocks.push(raw.into_block());
            continue;
        }
        if let Some(cell) = try_cache_cell(bytes, i) {
            cache_cells.push(cell);
        } else if let Some(entry) = try_directory_entry(bytes, i) {
            directory.push(entry);
        }
        i += 1;
    }

    Ok(ContainerScan {
        source_image: bytes.to_vec(),
        version,
        blocks,
        directory,
        cache_cells,
    })
}

/// Validate a marker hit as a block and, on success, route its DEFLATE payload
/// through the platform expander, register the decompressed `Transform` space,
/// and register each nested Parasolid stream.
///
/// `ExpandSpec::Exact` bounds the inflated output during the probe, so a corrupt
/// frame cannot inflate a bomb before validation. The CRC-32 and declared length
/// are validated from the streamed output before finalize, so a marker hit that
/// is not a real block registers no space and the writer is dropped. Returns
/// `Ok(None)` for any marker hit that does not frame or validate as a block — the
/// probe idiom (§3.3): the caller tries the cache-cell and directory framings
/// next. The only propagated error is an unswallowable `ResourceLimit` (§4.7).
fn admit_block(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    bytes: &[u8],
    off: usize,
) -> Result<Option<RawBlock>, CodecError> {
    let (Some(type_id), Some(crc), Some(comp_sz), Some(uncomp_sz), Some(pre_sz)) = (
        u32_le(bytes, off + 6),
        u32_le(bytes, off + 10),
        u32_le(bytes, off + 14),
        u32_le(bytes, off + 18),
        u32_le(bytes, off + 22),
    ) else {
        return Ok(None);
    };
    let comp = comp_sz as usize;
    let pre = pre_sz as usize;
    let uncomp = uncomp_sz as usize;
    // `MAX_UNCOMP`: deployment ceiling (see its definition), enforced here as
    // dual defense behind the expander's decompression envelope.
    if comp == 0 || uncomp == 0 || uncomp > MAX_UNCOMP {
        return Ok(None);
    }
    let payload_start = off + BLOCK_HEADER_LEN + pre;
    let Some(payload_end) = payload_start.checked_add(comp) else {
        return Ok(None);
    };
    let Some(source) = root.child(payload_start, payload_end) else {
        return Ok(None);
    };

    let mut writer = match ctx.begin_expand(source, ExpandSpec::Exact(uncomp_sz as u64)) {
        Ok(writer) => writer,
        Err(e) => return probe_or_propagate(e),
    };
    let mut hasher = crc32fast::Hasher::new();
    let mut decoder = flate2::read::DeflateDecoder::new(source.window());
    let mut chunk = [0u8; EXPAND_CHUNK];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                hasher.update(&chunk[..read]);
                if let Err(e) = writer.write(&chunk[..read]) {
                    return probe_or_propagate(e);
                }
            }
            // Corrupt DEFLATE stream: not a block, try the next interpretation.
            Err(_) => return Ok(None),
        }
    }
    if writer.written() != uncomp_sz as u64 || hasher.finalize() != crc {
        // Not a valid block; dropping the writer registers no space.
        return Ok(None);
    }
    let (_space, view) = match writer.finalize() {
        Ok(pair) => pair,
        Err(e) => return probe_or_propagate(e),
    };
    let inflated = view.window();

    let preamble = bytes
        .get(off + BLOCK_HEADER_LEN..payload_start)
        .unwrap_or(&[]);
    let section = nibble_swap_name(preamble);
    let located_streams = crate::parasolid::extract_streams_with_offsets(inflated);
    register_parasolid_spaces(ctx, view, inflated, &located_streams)?;
    let ps_stream_offsets = located_streams.iter().map(|(offset, _)| *offset).collect();
    let ps_streams = located_streams
        .into_iter()
        .map(|(_, stream)| stream)
        .collect::<Vec<_>>();
    let ps_stream = ps_streams.first().cloned();
    let family = if ps_streams.is_empty() {
        payload_family(inflated)
    } else {
        "parasolid"
    };

    Ok(Some(RawBlock {
        offset: off,
        type_id,
        comp_sz,
        uncomp_sz,
        preamble_len: pre,
        section,
        family,
        payload: inflated.to_vec(),
        ps_stream,
        ps_streams,
        ps_stream_offsets,
    }))
}

/// Map an expander error hit during the block probe: a fused `ResourceLimit` is
/// unswallowable and propagates (§4.7); any other error means this marker hit is
/// not a valid block, so the probe reports `NoMatch`.
fn probe_or_propagate(e: CodecError) -> Result<Option<RawBlock>, CodecError> {
    match e {
        CodecError::ResourceLimit(_) => Err(e),
        _ => Ok(None),
    }
}

/// Register each nested Parasolid stream in the runtime space graph. A stream
/// carried directly in the block payload is a `Slice` alias of the block's
/// decompressed space; a wrapped stream whose bytes are a nested zlib member is a
/// `Transform` derived space (§10 Phase 1C). Each nested read is framed as a view
/// of the block space, never a raw length (§10 Phase 1B).
fn register_parasolid_spaces(
    ctx: &DecodeContext<'_>,
    block: View<'_>,
    inflated: &[u8],
    streams: &[(usize, Vec<u8>)],
) -> Result<(), CodecError> {
    for (offset, stream) in streams {
        let Some(end) = offset.checked_add(stream.len()) else {
            continue;
        };
        if inflated.get(*offset..end) == Some(stream.as_slice()) {
            // Direct stream: a zero-copy Slice of the block space.
            ctx.register_slice(
                block,
                ByteRange {
                    start: *offset as u64,
                    end: end as u64,
                },
            )?;
            continue;
        }
        // Wrapped stream: the bytes at `offset` are a compressed member and
        // `stream` is its decompression. Record it as a Transform whose input is
        // the block region from the member offset, charging the decompressed
        // bytes through the writer.
        let Some(input) = block.child(*offset, block.end()) else {
            continue;
        };
        let mut writer =
            ctx.begin_derived_space(&[input], DerivedKind::Transform(TransformKind::Decompress))?;
        writer.write(stream)?;
        writer.finalize()?;
    }
    Ok(())
}

/// Scan an in-memory `.sldprt` image.
///
/// Truncated input produces a scan containing every structure that could be
/// validated; missing outer-header bytes yield version zero.
pub fn scan_bytes(bytes: &[u8]) -> ContainerScan {
    let version = cadmpeg_ir::be::u32_at(bytes, 4).unwrap_or(0);

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
/// [`cadmpeg_ir::CodecEntry::inspect`].
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

    let mut notes = vec![format!(
        "outer version word: 0x{:08x}; {} CRC-validated block(s), {} tail-directory \
         entry/entries, {} cache-cell(s)",
        scan.version,
        scan.blocks.len(),
        scan.directory.len(),
        scan.cache_cells.len()
    )];
    match select_active_parasolid(scan) {
        Some((b, sch)) => notes.push(format!(
            "active Parasolid B-rep candidate: {} ({} bytes, schema {})",
            b.section
                .clone()
                .unwrap_or_else(|| format!("block@{}", b.offset)),
            b.uncomp_sz,
            sch.schema
        )),
        None => notes.push(
            "no Parasolid partition/deltas stream located; B-rep decode will be container-only"
                .to_string(),
        ),
    }
    notes.push(
        "container-level enumeration; run `decode` to locate the Parasolid stream and build the \
         B-rep graph from its typed topology and analytic carriers"
            .to_string(),
    );

    ContainerSummary {
        format: "sldprt".to_string(),
        container_kind: "sldprt-blocks".to_string(),
        entries,
        notes,
    }
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
