// SPDX-License-Identifier: Apache-2.0
//! `.sldprt` outer container: block framing, cache-cell grid, and the tail
//! section directory.
//!
//! A `.sldprt` opens with an 8-byte header (`file_id`, then a big-endian
//! `version` word) and continues as a sequence of compressed blocks. Each block
//! is introduced by the marker `14 00 06 00 08 00` and carries a raw-DEFLATE
//! payload whose CRC-32 is stored in the frame. The same marker is reused by two
//! non-payload structures: a fixed-cell section-index grid (the "cache cells")
//! and the tail section directory. This module locates every marker, classifies
//! each hit against the frame's own validation gates, decompresses the real
//! blocks, and decodes the OPC section names carried (nibble-swapped) in block
//! preambles and directory entries.

use std::collections::BTreeMap;
use std::io::Read;

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use sha2::{Digest, Sha256};

/// The block/cache/directory marker that introduces every framed structure.
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
}

/// The decompressed-payload families a block can carry, keyed off the first
/// bytes of the inflated payload (spec §3).
pub fn payload_family(payload: &[u8]) -> &'static str {
    if payload.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        "png-preview"
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

/// Byte offset of the `PS\0\0` Parasolid stream signature within a payload, if
/// present near the start. The plain form sits at 0; wrapped forms carry a small
/// prefix, so a short leading window is scanned.
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

/// Decode an OPC section name from its stored (nibble-swapped) bytes: each byte
/// has its high and low nibble exchanged. Returns `None` if the result is not
/// fully printable ASCII (the spec's name gate).
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

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// A CRC-validated compressed block with its decompressed payload.
#[derive(Debug, Clone)]
pub struct Block {
    /// Byte offset of the marker in the file.
    pub offset: usize,
    /// Frame `type_id`.
    pub type_id: u32,
    /// Compressed payload length.
    pub comp_sz: u32,
    /// Decompressed payload length (validated to equal `payload.len()`).
    pub uncomp_sz: u32,
    /// OPC section name decoded from the preamble, when printable.
    pub section: Option<String>,
    /// Decompressed-payload family (spec §3).
    pub family: &'static str,
    /// The decompressed payload bytes.
    pub payload: Vec<u8>,
    /// The extracted Parasolid `PS\0\0` stream, when this block carries one
    /// (plain, wrapped, or nested). `None` for non-Parasolid blocks.
    pub ps_stream: Option<Vec<u8>>,
    /// Every Parasolid stream carried by this block.
    pub ps_streams: Vec<Vec<u8>>,
}

/// A tail section-directory entry naming one OPC part.
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    /// Byte offset of the marker.
    pub offset: usize,
    /// Frame `type_id`.
    pub type_id: u32,
    /// The section's stored/uncompressed size.
    pub size: u32,
    /// OPC part name (nibble-swapped).
    pub name: String,
}

/// A cache-cell section-index grid entry (not a compressed payload).
#[derive(Debug, Clone)]
pub struct CacheCell {
    /// Byte offset of the marker.
    pub offset: usize,
    /// The logical cell size `L`.
    pub logical_len: u32,
    /// OPC section name (nibble-swapped).
    pub name: String,
}

/// Everything read from the outer container, shared by `inspect` and `decode`.
pub struct ContainerScan {
    /// Complete source image for exact passthrough writing.
    pub source_image: Vec<u8>,
    /// The big-endian outer `version` word (`0x00000004` in known files).
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

/// Whether a byte prefix looks like a `.sldprt`: the block marker appears within
/// the leading window, after the 8-byte outer header. `.sldprt` files are not
/// OLE2 at the outer level, so the marker — not a compound-file magic — is the
/// signal.
pub fn looks_like_sldprt(prefix: &[u8]) -> bool {
    if prefix.len() < OUTER_HEADER_LEN + MARKER.len() {
        return false;
    }
    prefix[OUTER_HEADER_LEN..]
        .windows(MARKER.len())
        .any(|w| w == MARKER)
}

/// Read the whole file and classify every marker hit. Blocks are validated by
/// raw-DEFLATE round-trip + CRC-32; cache cells by the relational size
/// invariant; directory entries by a printable nibble-swapped name in the tail
/// frame. A marker that matches none is ignored (it is payload noise).
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(CodecError::Io)?;
    Ok(scan_bytes(&bytes))
}

/// Classify a whole `.sldprt` byte image. Split out so tests can drive it from a
/// synthetic buffer without a reader.
pub fn scan_bytes(bytes: &[u8]) -> ContainerScan {
    let version = u32::from_be_bytes([
        *bytes.get(4).unwrap_or(&0),
        *bytes.get(5).unwrap_or(&0),
        *bytes.get(6).unwrap_or(&0),
        *bytes.get(7).unwrap_or(&0),
    ]);

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
    let ps_streams = crate::parasolid::extract_streams(&inflated);
    let ps_stream = ps_streams.first().cloned();
    let family = if !ps_streams.is_empty() {
        "parasolid"
    } else {
        payload_family(&inflated)
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
/// printable nibble-swapped name (spec §2.2).
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
/// printable nibble-swapped name (spec §2.3).
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

/// Build a [`ContainerSummary`] enumerating blocks, directory entries, and the
/// cache grid, with the active Parasolid partition candidate noted.
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

/// Choose the block carrying the authoritative solid B-rep.
///
/// Several blocks carry a Parasolid stream, but only the active
/// `Config-0-Partition` holds the final solid. The `Config-0-GhostPartition` is
/// a superseded transmit stub and `Config-0-ResolvedFeatures` is the feature
/// lane (2D input profiles), so both are ranked below a real partition (spec
/// §3). Candidates are scored by section name and stream size; the highest wins,
/// with a Parasolid block of any kind as a last resort.
pub fn select_active_parasolid(
    scan: &ContainerScan,
) -> Option<(&Block, crate::parasolid::StreamHeader)> {
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
        } else if name.contains("deltas") || desc.contains("deltas") {
            score += 50_000;
        }

        if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
            best = Some((score, b, sch));
        }
    }
    best.map(|(_, b, sch)| (b, sch))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
