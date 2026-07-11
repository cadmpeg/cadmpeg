// SPDX-License-Identifier: Apache-2.0
//! `V5_CFV2` container parsing and logical-stream reconstruction.
//!
//! A `CATPart` begins with `V5_CFV2\0` and a big-endian outer directory
//! offset/length pair. Nested files contain a `CATIA_V5 CB0001` directory that
//! maps names such as `MainDataStream`, `SurfacicReps`, and `Header` to physical
//! extents. [`brep_stream`] reconstructs the B-rep buffer from the largest
//! `MainDataStream` and `SurfacicReps` descriptors in logical-offset order.
//!
//! [`scan`] reads the file, parses available directories, reconstructs the
//! stream, and records the structural census used to select a
//! [`crate::variant::Variant`]. [`summarize`] converts the scan into the
//! container view returned by codec inspection.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};

use crate::variant::Variant;

/// The outer and inner container magic.
pub const OUTER_MAGIC: &[u8; 8] = b"V5_CFV2\0";
/// The nested-container stream-directory magic.
pub const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";

/// Standard-nested BREP-spine markers used for variant identification.
const FBB_MARKER: &[u8; 4] = &[0x30, 0x04, 0x04, 0xff];
const EDGE_DELIMITER: &[u8; 8] = &[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_MARKER: &[u8; 3] = &[0x05, 0x08, 0x01];
const A9_MARKER: &[u8; 2] = &[0xa9, 0x03];
const E5_MARKER: &[u8; 3] = &[0xe5, 0x0d, 0x03];

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// A named logical stream catalogued by the inner directory.
    pub const STREAM: &str = "stream";
}

/// One physical extent of a logical stream. `phys_off` is measured from the inner
/// magic (absolute file offset = `inner + phys_off`).
#[derive(Debug, Clone)]
pub struct Extent {
    /// Physical byte offset from the inner `V5_CFV2` magic (absolute file
    /// offset = `inner + phys_off`).
    pub phys_off: u32,
    /// Physical byte length of this extent.
    pub phys_len: u32,
    /// Logical byte length; validated equal to `phys_len` ([spec §3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)).
    pub log_len: u32,
    /// Logical byte offset within the reconstructed stream; validated
    /// cumulative from `0` across a descriptor's extents ([spec §3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)).
    pub log_off: u32,
    /// Raw extent-struct flags word; meaning not decoded further.
    pub flags: u32,
}

/// One catalogued logical stream.
#[derive(Debug, Clone)]
pub struct Descriptor {
    /// UTF-16LE ASCII name (`MainDataStream`, `SurfacicReps`, …).
    pub name: String,
    /// Offset of the descriptor header within the directory region.
    pub desc_offset: usize,
    /// Logical stream length (equals the sum of extent `log_len`s).
    pub logical_length: u32,
    /// Physical extents, in `log_off` order.
    pub extents: Vec<Extent>,
}

/// The parsed inner sub-container directory.
#[derive(Debug, Clone)]
pub struct InnerDir {
    /// File offset of the inner `V5_CFV2` magic.
    pub inner: usize,
    /// File offset of the `CATIA_V5 CB0001` directory.
    pub dir_offset: usize,
    /// Directory region length (`B`).
    pub dir_length: u32,
    /// Catalogued streams.
    pub descriptors: Vec<Descriptor>,
}

/// Census counts used for variant identification and reporting.
#[derive(Debug, Clone, Default)]
pub struct Census {
    /// Contiguous stride-8 `30 04 04 ff` FBB runs in the BREP stream.
    pub fbb_runs: usize,
    /// `10 24 04 ff ff 00 00 00` standard edge-table delimiters in the BREP stream.
    pub edge_delimiters: usize,
    /// `05 08 01` vertex-record signatures in the BREP stream.
    pub vertex_markers: usize,
    /// `a9 03` record-family markers in the whole file.
    pub a9_markers: usize,
    /// `e5 0d 03` record-family markers in the whole file.
    pub e5_markers: usize,
}

/// Everything read from a `.CATPart`, shared by `inspect` and `decode`.
pub struct ContainerScan {
    /// The whole file image.
    pub data: Vec<u8>,
    /// Outer directory offset (big-endian, from `+8`).
    pub outer_dir_offset: u32,
    /// Outer directory length (big-endian, from `+12`).
    pub outer_dir_length: u32,
    /// Parsed inner directory, when the file is nested and cataloguable.
    pub inner: Option<InnerDir>,
    /// Reconstructed BREP stream (largest `MainDataStream` + `SurfacicReps`).
    pub brep: Option<Vec<u8>>,
    /// Record-family census.
    pub census: Census,
    /// Identified storage variant.
    pub variant: Variant,
}

/// Whether a byte prefix is a `.CATPart`: the `V5_CFV2\0` outer magic is unique
/// to Dassault's container and is a conclusive signal on its own.
pub fn looks_like_catia(prefix: &[u8]) -> bool {
    prefix.starts_with(OUTER_MAGIC)
}

fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// Count non-overlapping stride-8 runs of the FBB marker and total marker hits.
/// Returns `run_count`. The number of maximal contiguous groups is the
/// documented body count, but for variant detection the presence of any run is
/// what matters, so this counts every stride-8 marker occurrence.
fn count_stride8_fbb(body: &[u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i + 4 <= body.len() {
        if &body[i..i + 4] == FBB_MARKER {
            count += 1;
            i += 8;
        } else {
            i += 1;
        }
    }
    count
}

fn count_subslice(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// Parse the nested-container stream directory by the self-consistency scan
/// documented in the format spec ([§3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)). Returns `None` when there is no nested
/// container or no parseable directory (the non-nested `a9 03` variant, and the
/// contiguous-body exception whose directory catalogues no BREP streams).
pub fn parse_stream_directory(data: &[u8]) -> Option<InnerDir> {
    if data.len() < 16 {
        return None;
    }
    let inner = find_subslice(data, OUTER_MAGIC, OUTER_MAGIC.len())?;
    if inner + 16 > data.len() {
        return None;
    }
    let a = u32_be(data, inner + 8)? as usize;
    let b = u32_be(data, inner + 12)?;
    let dir_offset = inner + a;
    if dir_offset + 16 > data.len() || &data[dir_offset..dir_offset + 16] != DIR_MAGIC {
        return None;
    }
    let b_usize = b as usize;
    if b == 0 || dir_offset + b_usize > data.len() {
        return None;
    }
    let dirbuf = &data[dir_offset..dir_offset + b_usize];
    let file_len = data.len();
    let mut descriptors = Vec::new();

    // At each candidate extent-count field, validate every extent and the
    // descriptor-header logical length; a candidate that validates fully is a
    // real descriptor. The extent count sits at `desc_offset + 0x50`.
    let mut o = 0usize;
    while o + 4 <= dirbuf.len() {
        let Some(k) = u32_be(dirbuf, o).map(|value| value as usize) else {
            break;
        };
        if (1..=64).contains(&k) && o + 4 + 20 * k <= dirbuf.len() {
            if let Some((extents, cum)) = parse_extents(dirbuf, o, k, inner, file_len) {
                if cum > 0 && o >= 0x50 {
                    let ds = o - 0x50;
                    let logical_length = u32_be(dirbuf, ds + 0x0c).unwrap_or(0);
                    if logical_length as usize == cum {
                        descriptors.push(Descriptor {
                            name: descriptor_name(dirbuf, ds),
                            desc_offset: ds,
                            logical_length,
                            extents,
                        });
                    }
                }
            }
        }
        o += 1;
    }

    if descriptors.is_empty() {
        return None;
    }
    Some(InnerDir {
        inner,
        dir_offset,
        dir_length: b,
        descriptors,
    })
}

/// Validate the `k` 20-byte extent structs beginning at `o + 4`; returns the
/// extents and their cumulative logical length, or `None` if any extent fails a
/// gate (`log_off` cumulative from 0, `log_len == phys_len`, physically in range).
fn parse_extents(
    dirbuf: &[u8],
    o: usize,
    k: usize,
    inner: usize,
    file_len: usize,
) -> Option<(Vec<Extent>, usize)> {
    let mut extents = Vec::with_capacity(k);
    let mut cum: usize = 0;
    for i in 0..k {
        let base = o + 4 + 20 * i;
        let phys_off = u32_be(dirbuf, base)?;
        let phys_len = u32_be(dirbuf, base + 4)?;
        let log_len = u32_be(dirbuf, base + 8)?;
        let log_off = u32_be(dirbuf, base + 12)?;
        let flags = u32_be(dirbuf, base + 16)?;
        if phys_len == 0
            || inner + phys_off as usize + phys_len as usize > file_len
            || log_off as usize != cum
            || log_len != phys_len
        {
            return None;
        }
        cum += log_len as usize;
        extents.push(Extent {
            phys_off,
            phys_len,
            log_len,
            log_off,
            flags,
        });
    }
    Some((extents, cum))
}

/// Read the UTF-16LE ASCII stream name from a descriptor header region: the
/// longest run of printable ASCII characters each followed by a `0x00` high byte,
/// searched in the window preceding the extent-count field.
fn descriptor_name(dirbuf: &[u8], ds: usize) -> String {
    let start = ds.saturating_sub(40);
    let window = &dirbuf[start..ds + 0x50.min(dirbuf.len() - ds)];
    let mut best = String::new();
    let mut i = 0;
    while i + 1 < window.len() {
        let mut chars = String::new();
        let mut j = i;
        while j + 1 < window.len() && (0x20..0x7f).contains(&window[j]) && window[j + 1] == 0 {
            chars.push(window[j] as char);
            j += 2;
        }
        if chars.len() >= 3 {
            if chars.len() > best.len() {
                best = chars;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    best
}

/// Concatenate a logical stream's physical extents in `log_off` order.
pub fn reconstruct_logical_stream(data: &[u8], descriptor: &Descriptor, inner: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(descriptor.logical_length as usize);
    for e in &descriptor.extents {
        let start = inner + e.phys_off as usize;
        let end = start + e.phys_len as usize;
        if end <= data.len() {
            out.extend_from_slice(&data[start..end]);
        }
    }
    out
}

/// Reconstruct the logical BREP buffer: the largest `MainDataStream` followed by
/// the largest `SurfacicReps` ([spec §3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)). Both are required. A directory that
/// catalogues the BREP body carries both a substantial `MainDataStream` and a
/// `SurfacicReps`; the contiguous-body exception has neither and returns `None`.
pub fn brep_stream(data: &[u8], dir: &InnerDir) -> Option<Vec<u8>> {
    let main = dir
        .descriptors
        .iter()
        .filter(|d| d.name == "MainDataStream")
        .max_by_key(|d| d.logical_length)?;
    let surf = dir
        .descriptors
        .iter()
        .filter(|d| d.name.contains("Surf"))
        .max_by_key(|d| d.logical_length)?;
    let mut out = reconstruct_logical_stream(data, main, dir.inner);
    out.extend(reconstruct_logical_stream(data, surf, dir.inner));
    Some(out)
}

fn find_subslice(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Identify the storage variant from container-level evidence ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#1-variant-families)).
///
/// The identification is intentionally structural: standard-nested requires an
/// FBB spine plus the standard edge-table delimiter; FBB-only requires an FBB
/// spine without that delimiter; zero-entity requires no nested container and an
/// `a9 03` family; the object-stream / E5 families are named from their record
/// census when no FBB spine is present. Anything that matches no invariant is
/// [`Variant::Unknown`].
fn identify_variant(inner: Option<&InnerDir>, brep: Option<&[u8]>, census: &Census) -> Variant {
    match (inner, brep) {
        // No nested container at all.
        (None, _) => {
            if census.a9_markers > 0 {
                Variant::ZeroEntity
            } else {
                Variant::Unknown
            }
        }
        // Nested container, but its directory catalogues no BREP body.
        (Some(_), None) => Variant::InnerNoDirectory,
        (Some(_), Some(_)) => {
            if census.fbb_runs > 0 {
                if census.edge_delimiters > 0 {
                    Variant::StandardNested
                } else {
                    Variant::FbbOnly
                }
            } else if census.e5_markers > 0 {
                // An E5 family without an FBB spine identifies the E5 stream
                // variant. Classification does not require a coherent topology walk.
                Variant::E5Stream
            } else {
                Variant::FloatPackedInnerNoFbb
            }
        }
    }
}

/// Read the whole file and identify its variant, reconstructing the BREP stream
/// when the file is a cataloguable nested container.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(CodecError::Io)?;
    Ok(scan_bytes(data))
}

/// Identify a whole `.CATPart` byte image. Split out so tests drive it from a
/// synthetic buffer without a reader.
pub fn scan_bytes(data: Vec<u8>) -> ContainerScan {
    let outer_dir_offset = u32_be(&data, 8).unwrap_or(0);
    let outer_dir_length = u32_be(&data, 12).unwrap_or(0);

    let inner = parse_stream_directory(&data);
    let brep = inner.as_ref().and_then(|dir| brep_stream(&data, dir));

    let mut census = Census {
        a9_markers: count_subslice(&data, A9_MARKER),
        e5_markers: count_subslice(&data, E5_MARKER),
        ..Default::default()
    };
    if let Some(b) = &brep {
        census.fbb_runs = count_stride8_fbb(b);
        census.edge_delimiters = count_subslice(b, EDGE_DELIMITER);
        census.vertex_markers = count_subslice(b, VERTEX_MARKER);
    }

    let variant = identify_variant(inner.as_ref(), brep.as_deref(), &census);

    ContainerScan {
        data,
        outer_dir_offset,
        outer_dir_length,
        inner,
        brep,
        census,
        variant,
    }
}

/// Build a [`ContainerSummary`] enumerating the inner directory's named streams
/// and the identified variant.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let mut entries = Vec::new();

    if let Some(dir) = &scan.inner {
        for d in &dir.descriptors {
            let mut attributes = BTreeMap::new();
            attributes.insert("desc_offset".to_string(), d.desc_offset.to_string());
            attributes.insert("extent_count".to_string(), d.extents.len().to_string());
            let phys: u64 = d.extents.iter().map(|e| e.phys_len as u64).sum();
            entries.push(ContainerEntry {
                name: if d.name.is_empty() {
                    format!("stream@{}", d.desc_offset)
                } else {
                    d.name.clone()
                },
                role: role::STREAM.to_string(),
                compression: "none".to_string(),
                compressed_size: phys,
                uncompressed_size: d.logical_length as u64,
                attributes,
            });
        }
    }

    let mut notes = vec![format!(
        "outer V5_CFV2 container: directory offset {} + length {} = {} (file size {}); variant: {}",
        scan.outer_dir_offset,
        scan.outer_dir_length,
        scan.outer_dir_offset as u64 + scan.outer_dir_length as u64,
        scan.data.len(),
        scan.variant.description(),
    )];

    match &scan.inner {
        Some(dir) => notes.push(format!(
            "nested V5_CFV2 at file offset {} with a CATIA_V5 CB0001 directory of {} stream(s)",
            dir.inner,
            dir.descriptors.len()
        )),
        None => notes.push(
            "no nested V5_CFV2 sub-container (outer-preamble record families only)".to_string(),
        ),
    }

    if scan.brep.is_some() {
        notes.push(format!(
            "reconstructed BREP stream from MainDataStream + SurfacicReps: {} FBB run(s), {} \
             vertex record(s), {} edge-table delimiter(s)",
            scan.census.fbb_runs, scan.census.vertex_markers, scan.census.edge_delimiters
        ));
    }
    if scan.census.a9_markers > 0 || scan.census.e5_markers > 0 {
        notes.push(format!(
            "record-family census: {} a9 03, {} e5 0d 03",
            scan.census.a9_markers, scan.census.e5_markers
        ));
    }
    notes.push(
        "container-level enumeration; run `decode` to build geometry from the standard-nested \
         BREP stream (other variants are container-only)"
            .to_string(),
    );

    ContainerSummary {
        format: "catia".to_string(),
        container_kind: "v5-cfv2".to_string(),
        entries,
        notes,
    }
}
