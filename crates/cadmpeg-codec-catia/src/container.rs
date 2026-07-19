// SPDX-License-Identifier: Apache-2.0
//! `V5_CFV2` container parsing and logical-stream reconstruction.
//!
//! A `CATPart` begins with `V5_CFV2\0` and a big-endian outer directory
//! offset/length pair. Nested files contain a `CATIA_V5 CB0001` directory that
//! maps names such as `MainDataStream`, `SurfacicReps`, and `Header` to physical
//! extents. [`brep_stream`] reconstructs the B-rep buffer from the largest
//! `MainDataStream` and `SurfacicReps` descriptors in logical-offset order.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_ir::be::u32_at as u32_be;
use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::decode::{ByteRange, DecodeContext, DerivedKind, View};

use crate::variant::Variant;

/// Allocation charge for one registered extent.
const PER_EXTENT_GRAPH_BYTES: u64 = 256;

/// Maximum physical extents per catalogued descriptor.
const MAX_EXTENTS_PER_DESCRIPTOR: usize = 64;

/// The outer and inner container magic.
pub const OUTER_MAGIC: &[u8; 8] = b"V5_CFV2\0";
/// The nested-container stream-directory magic.
pub const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";
/// Marker opening a FINJPL named outer-body segment.
pub const FINJPL_MARKER: &[u8; 8] = b"FINJPL  ";

/// Semantic family of a FINJPL segment's big-endian type word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinjplKind {
    /// `CATStorageProperty` carrier.
    Storage,
    /// `CATProjectFlags` or `CATSummaryInformation` carrier.
    ProjectFlags,
    /// Manufacturer, OSMX, preview, or other named block.
    Other,
}

/// One FINJPL segment bounded by the next marker or the supplied body end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinjplSegment {
    /// Complete byte range beginning at the marker.
    pub range: Range<usize>,
    /// Big-endian type word immediately following the marker.
    pub type_word: u32,
    /// Classified type family.
    pub kind: FinjplKind,
}

/// Split FINJPL segments within a bounded outer-body range.
#[must_use]
pub fn finjpl_segments(data: &[u8], body_start: usize, body_end: usize) -> Vec<FinjplSegment> {
    let end = body_end.min(data.len());
    if body_start >= end {
        return Vec::new();
    }
    let positions: Vec<usize> = data[body_start..end]
        .windows(FINJPL_MARKER.len())
        .enumerate()
        .filter_map(|(relative, bytes)| (bytes == FINJPL_MARKER).then_some(body_start + relative))
        .collect();
    positions
        .iter()
        .enumerate()
        .filter_map(|(index, &pos)| {
            let type_word = u32_be(data, pos + FINJPL_MARKER.len())?;
            let segment_end = positions.get(index + 1).copied().unwrap_or(end);
            let kind = match type_word {
                0x0000_0080 | 0x0000_0082 | 0x0000_0084 | 0x0000_0086 | 0x0000_008e
                | 0x0000_0090 | 0x0000_0092 => FinjplKind::Storage,
                0x0101_0001..=0x0101_0003 => FinjplKind::ProjectFlags,
                _ => FinjplKind::Other,
            };
            Some(FinjplSegment {
                range: pos..segment_end,
                type_word,
                kind,
            })
        })
        .collect()
}

/// Locate the coherent E5 record stream in the outer-body preamble or a FINJPL segment.
///
/// A candidate must contain at least ten stride-valid records. The preamble wins when
/// coherent; otherwise the segment with the largest valid walk wins, with storage type
/// `0x0000_008e` breaking ties.
#[must_use]
pub fn e5_record_stream(data: &[u8]) -> Option<Range<usize>> {
    if !data.starts_with(OUTER_MAGIC) {
        return None;
    }
    let directory_offset = usize::try_from(u32_be(data, 8)?).ok()?;
    let directory_length = usize::try_from(u32_be(data, 12)?).ok()?;
    if directory_offset.checked_add(directory_length)? != data.len()
        || directory_length >= data.len()
    {
        return None;
    }
    let first_finjpl = data[directory_length..]
        .windows(FINJPL_MARKER.len())
        .position(|bytes| bytes == FINJPL_MARKER)
        .map_or(data.len(), |relative| directory_length + relative);
    let preamble = directory_length..first_finjpl;
    if count_e5_records(&data[preamble.clone()]) >= MIN_COHERENT_E5_RECORDS {
        return Some(preamble);
    }

    finjpl_segments(data, directory_length, data.len())
        .into_iter()
        .filter_map(|segment| {
            let count = count_e5_records(&data[segment.range.clone()]);
            (count >= MIN_COHERENT_E5_RECORDS).then_some((
                count,
                segment.type_word == 0x0000_008e,
                segment.range,
            ))
        })
        .max_by_key(|(count, preferred, _)| (*count, *preferred))
        .map(|(_, _, range)| range)
}

fn count_e5_records(data: &[u8]) -> usize {
    let mut count = 0;
    let mut position = 0;
    while position + 13 <= data.len() {
        let Some(relative) = data[position..]
            .windows(E5_MARKER.len())
            .position(|bytes| bytes == E5_MARKER)
        else {
            break;
        };
        let record = position + relative;
        let Some(size) = data
            .get(record + 5..record + 7)
            .map(|bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
        else {
            break;
        };
        let Some(end) = record.checked_add(size + 13) else {
            break;
        };
        if end > data.len() {
            break;
        }
        count += 1;
        position = end;
    }
    count
}

/// Standard-nested BREP-spine markers used for variant identification.
const FBB_MARKER: &[u8; 4] = &[0x30, 0x04, 0x04, 0xff];
const EDGE_DELIMITER: &[u8; 8] = &[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_MARKER: &[u8; 3] = &[0x05, 0x08, 0x01];
const A9_MARKER: &[u8; 2] = &[0xa9, 0x03];
const E5_MARKER: &[u8; 3] = &[0xe5, 0x0d, 0x03];

/// Minimum stride-valid E5 records in a coherent E5 stream.
const MIN_COHERENT_E5_RECORDS: usize = 10;

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

impl Extent {
    /// Returns this extent's absolute non-empty range within the file.
    fn abs_range(&self, inner: usize, data_len: usize) -> Option<Range<usize>> {
        let start = inner.checked_add(self.phys_off as usize)?;
        let end = start.checked_add(self.phys_len as usize)?;
        (start < end && end <= data_len).then_some(start..end)
    }
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
pub struct ContainerScan<'a> {
    /// The whole file image, borrowed from the session root view.
    pub data: &'a [u8],
    /// Outer directory offset (big-endian, from `+8`).
    pub outer_dir_offset: u32,
    /// Outer directory length (big-endian, from `+12`).
    pub outer_dir_length: u32,
    /// Parsed inner directory, when the file is nested and cataloguable.
    pub inner: Option<InnerDir>,
    /// Reconstructed BREP stream (largest `MainDataStream` + `SurfacicReps`).
    pub brep: Option<View<'a>>,
    /// Record-family census.
    pub census: Census,
    /// Identified storage variant.
    pub variant: Variant,
}

impl<'a> ContainerScan<'a> {
    /// Bytes of the reconstructed logical BREP stream, when one was assembled.
    #[must_use]
    pub fn brep_bytes(&self) -> Option<&'a [u8]> {
        self.brep.map(View::window)
    }
}

/// Whether a byte prefix is a `.CATPart`: the `V5_CFV2\0` outer magic is unique
/// to Dassault's container and is a conclusive signal on its own.
pub fn looks_like_catia(prefix: &[u8]) -> bool {
    prefix.starts_with(OUTER_MAGIC)
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
        if (1..=MAX_EXTENTS_PER_DESCRIPTOR).contains(&k) && o + 4 + 20 * k <= dirbuf.len() {
            if let Some((extents, cum)) = parse_extents(dirbuf, o, k, inner, file_len) {
                if cum > 0 && o >= 0x50 {
                    let ds = o - 0x50;
                    if let Some(logical_length) = u32_be(dirbuf, ds + 0x0c) {
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
        }
        o += 1;
    }

    if descriptors.is_empty() {
        return None;
    }
    Some(InnerDir {
        inner,
        dir_offset,
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
    let mut extents = Vec::new();
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
    let mut out = Vec::new();
    for e in &descriptor.extents {
        if let Some(range) = e.abs_range(inner, data.len()) {
            out.extend_from_slice(&data[range]);
        }
    }
    out
}

/// Select the two BREP-body descriptors: the largest `MainDataStream` and the
/// largest `SurfacicReps`. Both are required. A directory that catalogues the
/// BREP body carries both; the contiguous-body exception has neither.
fn brep_descriptors(dir: &InnerDir) -> Option<(&Descriptor, &Descriptor)> {
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
    Some((main, surf))
}

/// Absolute file ranges of the BREP body's physical extents, in the order
/// [`brep_stream`] concatenates them (largest `MainDataStream` then largest
/// `SurfacicReps`, each in `log_off` order). These are the `Concat` segments of
/// the reconstructed logical stream.
pub(crate) fn brep_extent_ranges(data: &[u8], dir: &InnerDir) -> Option<Vec<Range<usize>>> {
    let (main, surf) = brep_descriptors(dir)?;
    let mut ranges = Vec::new();
    for descriptor in [main, surf] {
        for e in &descriptor.extents {
            if let Some(range) = e.abs_range(dir.inner, data.len()) {
                ranges.push(range);
            }
        }
    }
    Some(ranges)
}

/// Reconstruct the logical BREP buffer: the largest `MainDataStream` followed by
/// the largest `SurfacicReps` ([spec §3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)). Both are required. A directory that
/// catalogues the BREP body carries both a substantial `MainDataStream` and a
/// `SurfacicReps`; the contiguous-body exception has neither and returns `None`.
pub fn brep_stream(data: &[u8], dir: &InnerDir) -> Option<Vec<u8>> {
    let (main, surf) = brep_descriptors(dir)?;
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
fn identify_variant(
    inner: Option<&InnerDir>,
    brep: Option<&[u8]>,
    census: &Census,
    coherent_e5: bool,
) -> Variant {
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
            } else if coherent_e5 {
                Variant::E5Stream
            } else {
                Variant::FloatPackedInnerNoFbb
            }
        }
    }
}

/// Scans a `.CATPart` from a session root view.
pub fn scan_view<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    let data = root.window();
    ctx.charge_work(
        data.len() as u64,
        "catia_container_scan",
        Some(root.location()),
    )?;
    let outer_dir_offset = u32_be(data, 8).unwrap_or(0);
    let outer_dir_length = u32_be(data, 12).unwrap_or(0);
    let inner = parse_stream_directory(data);
    register_extent_spaces(ctx, root, inner.as_ref())?;
    let brep = build_brep_space(ctx, root, inner.as_ref())?;
    let brep_window = brep.as_ref().map(|v| v.window());
    ctx.charge_work(
        (data.len() as u64).saturating_mul(3),
        "catia_container_census",
        Some(root.location()),
    )?;
    if let Some(b) = brep_window {
        ctx.charge_work(
            (b.len() as u64).saturating_mul(3),
            "catia_brep_census",
            Some(root.location()),
        )?;
    }
    let (census, variant) = identify(data, inner.as_ref(), brep_window);
    Ok(ContainerScan {
        data,
        outer_dir_offset,
        outer_dir_length,
        inner,
        brep,
        census,
        variant,
    })
}

/// Registers catalogued extents as slices of the root space.
fn register_extent_spaces(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    inner: Option<&InnerDir>,
) -> Result<(), CodecError> {
    let Some(dir) = inner else {
        return Ok(());
    };
    let extent_count: u64 = dir.descriptors.iter().map(|d| d.extents.len() as u64).sum();
    ctx.charge_alloc(
        extent_count.saturating_mul(PER_EXTENT_GRAPH_BYTES),
        "catia_container_extents",
        Some(root.location()),
    )?;
    let len = root.window().len();
    for descriptor in &dir.descriptors {
        for e in &descriptor.extents {
            let Some(range) = e.abs_range(dir.inner, len) else {
                continue;
            };
            ctx.register_slice(
                root,
                ByteRange {
                    start: range.start as u64,
                    end: range.end as u64,
                },
            )?;
        }
    }
    Ok(())
}

/// Reassembles the BREP stream from its physical extents.
fn build_brep_space<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    inner: Option<&InnerDir>,
) -> Result<Option<View<'a>>, CodecError> {
    let Some(dir) = inner else {
        return Ok(None);
    };
    let Some(ranges) = brep_extent_ranges(root.window(), dir) else {
        return Ok(None);
    };
    let mut segments = Vec::new();
    for range in ranges {
        let Some(child) = root.child(range.start, range.end) else {
            return Ok(None);
        };
        segments.push(child);
    }
    let writer = ctx.begin_derived_space(&segments, DerivedKind::Concat)?;
    let (_space, view) = writer.finalize()?;
    Ok(Some(view))
}

/// Identifies a `.CATPart` without a decode context.
pub fn scan_bytes(data: &[u8]) -> ContainerScan<'_> {
    let outer_dir_offset = u32_be(data, 8).unwrap_or(0);
    let outer_dir_length = u32_be(data, 12).unwrap_or(0);

    let inner = parse_stream_directory(data);
    let brep = inner.as_ref().and_then(|dir| brep_stream(data, dir));
    let (census, variant) = identify(data, inner.as_ref(), brep.as_deref());

    ContainerScan {
        data,
        outer_dir_offset,
        outer_dir_length,
        inner,
        brep: None,
        census,
        variant,
    }
}

/// Computes the record-family census and storage variant.
fn identify(data: &[u8], inner: Option<&InnerDir>, brep: Option<&[u8]>) -> (Census, Variant) {
    let mut census = Census {
        a9_markers: count_subslice(data, A9_MARKER),
        e5_markers: count_subslice(data, E5_MARKER),
        ..Default::default()
    };
    if let Some(b) = brep {
        census.fbb_runs = count_stride8_fbb(b);
        census.edge_delimiters = count_subslice(b, EDGE_DELIMITER);
        census.vertex_markers = count_subslice(b, VERTEX_MARKER);
    }
    let variant = identify_variant(inner, brep, &census, e5_record_stream(data).is_some());
    (census, variant)
}

/// Build a [`ContainerSummary`] enumerating the inner directory's named streams
/// and the identified variant.
pub fn summarize(scan: &ContainerScan<'_>) -> ContainerSummary {
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
