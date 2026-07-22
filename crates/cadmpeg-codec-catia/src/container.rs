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
use std::ops::Range;

use cadmpeg_ir::be::u32_at as u32_be;
use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};

use crate::variant::Variant;

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
    /// Primary length-prefixed ASCII block name, when present.
    pub name: Option<String>,
}

/// One complete JPEG preview embedded in a summary-information segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    /// Exact file range from JPEG SOI through EOI.
    pub range: Range<usize>,
    /// Pixel width from the JPEG start-of-frame segment.
    pub width: u16,
    /// Pixel height from the JPEG start-of-frame segment.
    pub height: u16,
    /// Component count from the JPEG start-of-frame segment.
    pub components: u8,
}

/// CATIA application version stored by the summary-information record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastSaveVersion {
    /// CATIA generation number.
    pub version: u16,
    /// CATIA release number.
    pub release: u16,
    /// Installed service-pack number.
    pub service_pack: u16,
    /// Installed hot-fix number.
    pub hot_fix: u16,
    /// Source build-date string.
    pub build_date: String,
}

/// One external CATIA document named by a storage-property record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalReference {
    /// File offset of the length-prefixed target string.
    pub offset: usize,
    /// Referenced CATIA document name or path.
    pub target: String,
}

/// Split FINJPL segments within a bounded outer-body range.
#[must_use]
pub fn finjpl_segments(data: &[u8], body_start: usize, body_end: usize) -> Vec<FinjplSegment> {
    let end = body_end.min(data.len());
    if body_start >= end {
        return Vec::new();
    }
    let positions: Vec<usize> = memchr::memmem::find_iter(&data[body_start..end], FINJPL_MARKER)
        .map(|relative| body_start + relative)
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
                name: finjpl_primary_name(data, pos, segment_end),
            })
        })
        .collect()
}

fn finjpl_primary_name(data: &[u8], pos: usize, end: usize) -> Option<String> {
    let length = usize::try_from(u32_be(data, pos + 12)?).ok()?;
    let start = pos.checked_add(17)?;
    let name_end = start.checked_add(length)?;
    if data.get(pos + 16) != Some(&0) || name_end > end {
        return None;
    }
    let value = data.get(start..name_end)?;
    (!value.is_empty() && value.iter().all(|byte| matches!(byte, 0x20..=0x7e)))
        .then(|| std::str::from_utf8(value).ok().map(str::to_owned))?
}

/// Extract length-closed JPEG previews from `CATSummaryInformation` FINJPL
/// segments. JPEG marker framing supplies both dimensions and the exact image
/// boundary; incidental JPEG signatures outside this segment family are ignored.
#[must_use]
pub fn preview_images(data: &[u8]) -> Vec<PreviewImage> {
    let segments = finjpl_segments(data, 0, data.len());
    preview_images_in_segments(data, &segments)
}

fn preview_images_in_segments(data: &[u8], segments: &[FinjplSegment]) -> Vec<PreviewImage> {
    segments
        .iter()
        .filter(|segment| segment.type_word == 0x0101_0003)
        .filter_map(|segment| {
            let bytes = &data[segment.range.clone()];
            let mut candidates = bytes
                .windows(3)
                .enumerate()
                .filter(|(_, value)| *value == [0xff, 0xd8, 0xff])
                .filter_map(|(start, _)| {
                    jpeg_extent(bytes, start).map(|(end, width, height, components)| {
                        (start, end, width, height, components)
                    })
                });
            let (relative_start, relative_end, width, height, components) = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            Some(PreviewImage {
                range: segment.range.start + relative_start..segment.range.start + relative_end,
                width,
                height,
                components,
            })
        })
        .collect()
}

/// Decode the unique `LastSaveVersion` tuple from summary-information segments.
/// Repeated identical copies collapse to one value; conflicting copies reject
/// the version instead of selecting by position.
#[must_use]
#[cfg(test)]
pub fn last_save_version(data: &[u8]) -> Option<LastSaveVersion> {
    let segments = finjpl_segments(data, 0, data.len());
    last_save_version_in_segments(data, &segments)
}

fn last_save_version_in_segments(
    data: &[u8],
    segments: &[FinjplSegment],
) -> Option<LastSaveVersion> {
    let mut versions = segments
        .iter()
        .filter(|segment| segment.type_word == 0x0101_0003)
        .filter_map(|segment| parse_last_save_version(&data[segment.range.clone()]))
        .collect::<Vec<_>>();
    versions.dedup();
    (versions.len() == 1).then(|| versions.remove(0))
}

/// Enumerate exact `CATStorageProperty` external-document references from
/// project-flags segments.
#[must_use]
pub fn external_references(data: &[u8]) -> Vec<ExternalReference> {
    let segments = finjpl_segments(data, 0, data.len());
    external_references_in_segments(data, &segments)
}

fn external_references_in_segments(
    data: &[u8],
    segments: &[FinjplSegment],
) -> Vec<ExternalReference> {
    const STORAGE: &[u8] = b"\x34\x12CATStorageProperty";
    segments
        .iter()
        .filter(|segment| segment.kind == FinjplKind::ProjectFlags)
        .flat_map(|segment| {
            let bytes = &data[segment.range.clone()];
            bytes
                .windows(STORAGE.len())
                .enumerate()
                .filter_map(move |(relative, value)| {
                    (value == STORAGE).then_some(relative).and_then(|start| {
                        parse_external_reference(bytes, start).map(|mut reference| {
                            reference.offset += segment.range.start;
                            reference
                        })
                    })
                })
        })
        .collect()
}

fn parse_external_reference(data: &[u8], start: usize) -> Option<ExternalReference> {
    let mut at = start;
    (length_prefixed_ascii(data, &mut at)? == "CATStorageProperty").then_some(())?;
    (data.get(at..at + 6) == Some(&[0x80, 0x01, 0, 0, 0, 0])).then_some(())?;
    at += 6;
    (data.get(at..at + 9) == Some(&[0x22, 0x0c, 0, 0, 0, 0x34, 0x01, 0x01, 0x00])).then_some(())?;
    at += 9;
    (length_prefixed_ascii(data, &mut at)? == "CATUnicodeString").then_some(())?;
    (data.get(at..at + 6) == Some(&[0xa0, 0x02, 0, 0, 0, 0])).then_some(())?;
    at += 6;
    (length_prefixed_ascii(data, &mut at)? == "CATIA").then_some(())?;
    (data.get(at) == Some(&0x9f)).then_some(())?;
    at += 1;
    (data.get(at..at + 6) == Some(&[0xa0, 0x02, 0, 0, 0, 0])).then_some(())?;
    at += 6;
    let target_offset = at;
    let target = length_prefixed_ascii(data, &mut at)?;
    (data.get(at) == Some(&0x9f) && is_catia_document_name(&target)).then_some(())?;
    Some(ExternalReference {
        offset: target_offset,
        target,
    })
}

fn length_prefixed_ascii(data: &[u8], at: &mut usize) -> Option<String> {
    (data.get(*at) == Some(&0x34)).then_some(())?;
    let length = usize::from(*data.get(*at + 1)?);
    let start = (*at).checked_add(2)?;
    let end = start.checked_add(length)?;
    let value = data.get(start..end)?;
    *at = end;
    value
        .is_ascii()
        .then(|| std::str::from_utf8(value).ok().map(str::to_owned))?
}

fn is_catia_document_name(value: &str) -> bool {
    [".catpart", ".catproduct", ".catshape", ".cgr"]
        .iter()
        .any(|extension| value.to_ascii_lowercase().ends_with(extension))
}

fn parse_last_save_version(data: &[u8]) -> Option<LastSaveVersion> {
    let version = tagged_ascii(data, b"<Version>", b"/<Version>")?
        .parse()
        .ok()?;
    let release = tagged_ascii(data, b"<Release>", b"/<Release>")?
        .parse()
        .ok()?;
    let service_pack = tagged_ascii(data, b"<ServicePack>", b"/<ServicePack>")?
        .parse()
        .ok()?;
    let hot_fix = tagged_ascii(data, b"<HotFix>", b"/<HotFix>")?
        .parse()
        .ok()?;
    let build_date = tagged_ascii(data, b"<BuildDate>", b"/<BuildDate>")?;
    Some(LastSaveVersion {
        version,
        release,
        service_pack,
        hot_fix,
        build_date,
    })
}

fn tagged_ascii(data: &[u8], open: &[u8], close: &[u8]) -> Option<String> {
    let start = data.windows(open.len()).position(|value| value == open)? + open.len();
    let relative_end = data[start..]
        .windows(close.len())
        .position(|value| value == close)?;
    let value = data.get(start..start + relative_end)?;
    value
        .is_ascii()
        .then(|| std::str::from_utf8(value).ok().map(str::to_owned))?
}

fn jpeg_extent(data: &[u8], start: usize) -> Option<(usize, u16, u16, u8)> {
    if data.get(start..start + 2) != Some(&[0xff, 0xd8]) {
        return None;
    }
    let mut at = start + 2;
    let mut frame = None;
    let mut in_entropy = false;
    while at + 1 < data.len() {
        if data[at] != 0xff {
            if in_entropy {
                at += 1;
                continue;
            }
            return None;
        }
        while data.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *data.get(at)?;
        at += 1;
        if in_entropy && marker == 0x00 {
            continue;
        }
        if marker == 0xd9 {
            let (width, height, components) = frame?;
            return Some((at, width, height, components));
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
        if length < 2 {
            return None;
        }
        let payload = at + 2;
        let end = at.checked_add(length)?;
        if end > data.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 8 {
                return None;
            }
            let width = u16::from_be_bytes([data[payload + 3], data[payload + 4]]);
            let height = u16::from_be_bytes([data[payload + 1], data[payload + 2]]);
            let components = data[payload + 5];
            let expected_length = 8usize.checked_add(3usize.checked_mul(components.into())?)?;
            if width == 0
                || height == 0
                || components == 0
                || length != expected_length
                || frame.is_some()
            {
                return None;
            }
            frame = Some((width, height, components));
        }
        in_entropy = marker == 0xda;
        at = end;
    }
    None
}

/// Locate the coherent E5 record stream in the outer-body preamble or a FINJPL segment.
///
/// A candidate must contain at least ten stride-valid records. The preamble wins when
/// coherent; otherwise the segment with the largest valid walk wins, with storage type
/// `0x0000_008e` breaking ties.
#[must_use]
pub fn e5_record_stream(data: &[u8]) -> Option<Range<usize>> {
    let segments = finjpl_segments(data, 0, data.len());
    e5_record_stream_in_segments(data, &segments)
}

fn e5_record_stream_in_segments(data: &[u8], segments: &[FinjplSegment]) -> Option<Range<usize>> {
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
    if count_e5_records(&data[preamble.clone()]) >= 10 {
        return Some(preamble);
    }

    segments
        .iter()
        .filter(|segment| segment.range.start >= directory_length)
        .filter_map(|segment| {
            let count = count_e5_records(&data[segment.range.clone()]);
            (count >= 10).then_some((
                count,
                segment.type_word == 0x0000_008e,
                segment.range.clone(),
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

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// A named logical stream catalogued by the inner directory.
    pub const STREAM: &str = "stream";
    /// JPEG preview embedded in the outer summary-information segment.
    pub const PREVIEW: &str = "preview";
    /// Referenced CATIA document.
    pub const EXTERNAL_REFERENCE: &str = "external-reference";
    /// Named outer FINJPL block.
    pub const FINJPL_SEGMENT: &str = "finjpl-segment";
}

/// One physical extent of a logical stream. `phys_off` is measured from the
/// directory's physical storage base.
#[derive(Debug, Clone)]
pub struct Extent {
    /// Physical byte offset from the storage base. The base is zero for an
    /// outer directory and the nested magic offset for an inner directory.
    pub phys_off: u32,
    /// Physical byte length of this extent.
    pub phys_len: u32,
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

/// A parsed stream directory. `inner` is the physical storage base: zero for
/// the outer directory and the nested `V5_CFV2` offset for an inner directory.
#[derive(Debug, Clone)]
pub struct InnerDir {
    /// File offset of the inner `V5_CFV2` magic.
    pub inner: usize,
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
    /// Parsed outer stream directory. Its descriptor physical offsets are
    /// absolute because `inner == 0`.
    pub outer: Option<InnerDir>,
    /// Parsed inner directory, when the file is nested and cataloguable.
    pub inner: Option<InnerDir>,
    /// Reconstructed BREP stream (largest `MainDataStream` + `SurfacicReps`).
    pub brep: Option<Vec<u8>>,
    /// Exact JPEG previews extracted from summary-information framing.
    pub previews: Vec<PreviewImage>,
    /// Unique saved-by application version from summary information.
    pub last_save_version: Option<LastSaveVersion>,
    /// External CATIA documents named by storage properties.
    pub external_references: Vec<ExternalReference>,
    /// Every bounded outer FINJPL block in source order.
    pub finjpl_segments: Vec<FinjplSegment>,
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
    memchr::memmem::find_iter(haystack, needle).count()
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
    parse_directory_region(data, inner, dir_offset, b_usize)
}

/// Parse the outer `CATIA_V5 CB0001` stream directory. Physical extent offsets
/// in its descriptors are absolute file offsets.
#[must_use]
pub fn parse_outer_stream_directory(data: &[u8]) -> Option<InnerDir> {
    let dir_offset = usize::try_from(u32_be(data, 8)?).ok()?;
    let dir_length = usize::try_from(u32_be(data, 12)?).ok()?;
    (dir_offset.checked_add(dir_length)? == data.len()).then_some(())?;
    parse_directory_region(data, 0, dir_offset, dir_length)
}

fn parse_directory_region(
    data: &[u8],
    physical_base: usize,
    dir_offset: usize,
    dir_length: usize,
) -> Option<InnerDir> {
    if dir_length == 0
        || dir_offset.checked_add(dir_length)? > data.len()
        || data.get(dir_offset..dir_offset + 16) != Some(DIR_MAGIC)
    {
        return None;
    }
    let dirbuf = &data[dir_offset..dir_offset + dir_length];
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
            if let Some((extents, cum)) = parse_extents(dirbuf, o, k, physical_base, file_len) {
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
        inner: physical_base,
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
    physical_base: usize,
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
        // Presence-validate the trailing flags word without retaining it.
        u32_be(dirbuf, base + 16)?;
        if phys_len == 0
            || physical_base + phys_off as usize + phys_len as usize > file_len
            || log_off as usize != cum
            || log_len != phys_len
        {
            return None;
        }
        cum += log_len as usize;
        extents.push(Extent { phys_off, phys_len });
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
    // A logical stream cannot exceed the physical file; clamp the eager
    // reservation to the available bytes so a forged length cannot amplify it.
    let capacity = cadmpeg_ir::cursor::bounded_len(descriptor.logical_length as u64, 1, data.len())
        .unwrap_or(0);
    let mut out = Vec::with_capacity(capacity);
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
            if coherent_e5 {
                Variant::E5Stream
            } else if census.fbb_runs > 0 {
                if census.edge_delimiters > 0 {
                    Variant::StandardNested
                } else {
                    Variant::FbbOnly
                }
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

    let outer = parse_outer_stream_directory(&data);
    let inner = parse_stream_directory(&data);
    let brep = inner.as_ref().and_then(|dir| brep_stream(&data, dir));
    let finjpl_segments = finjpl_segments(&data, 0, data.len());
    let previews = preview_images_in_segments(&data, &finjpl_segments);
    let last_save_version = last_save_version_in_segments(&data, &finjpl_segments);
    let external_references = external_references_in_segments(&data, &finjpl_segments);

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

    let variant = identify_variant(
        inner.as_ref(),
        brep.as_deref(),
        &census,
        e5_record_stream_in_segments(&data, &finjpl_segments).is_some(),
    );

    ContainerScan {
        data,
        outer_dir_offset,
        outer_dir_length,
        outer,
        inner,
        brep,
        previews,
        last_save_version,
        external_references,
        finjpl_segments,
        census,
        variant,
    }
}

/// Build a [`ContainerSummary`] enumerating the outer and inner directories'
/// streams and the identified variant.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let mut entries = Vec::new();

    for (directory, dir) in [
        ("outer", scan.outer.as_ref()),
        ("inner", scan.inner.as_ref()),
    ] {
        let Some(dir) = dir else { continue };
        for d in &dir.descriptors {
            let mut attributes = BTreeMap::new();
            attributes.insert("directory".to_string(), directory.to_string());
            attributes.insert("desc_offset".to_string(), d.desc_offset.to_string());
            attributes.insert("extent_count".to_string(), d.extents.len().to_string());
            let phys: u64 = d.extents.iter().map(|e| e.phys_len as u64).sum();
            entries.push(ContainerEntry {
                name: if d.name.is_empty() {
                    format!("{directory}-stream@{}", d.desc_offset)
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
    for (index, preview) in scan.previews.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        attributes.insert("file_offset".to_string(), preview.range.start.to_string());
        attributes.insert("width".to_string(), preview.width.to_string());
        attributes.insert("height".to_string(), preview.height.to_string());
        attributes.insert("components".to_string(), preview.components.to_string());
        entries.push(ContainerEntry {
            name: format!("CATPreview#{index}"),
            role: role::PREVIEW.to_string(),
            compression: "jpeg".to_string(),
            compressed_size: (preview.range.end - preview.range.start) as u64,
            uncompressed_size: 0,
            attributes,
        });
    }
    for reference in &scan.external_references {
        let mut attributes = BTreeMap::new();
        attributes.insert("file_offset".to_string(), reference.offset.to_string());
        entries.push(ContainerEntry {
            name: reference.target.clone(),
            role: role::EXTERNAL_REFERENCE.to_string(),
            compression: "none".to_string(),
            compressed_size: 0,
            uncompressed_size: 0,
            attributes,
        });
    }
    for (index, segment) in scan.finjpl_segments.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        attributes.insert("file_offset".to_string(), segment.range.start.to_string());
        attributes.insert(
            "type_word".to_string(),
            format!("0x{:08x}", segment.type_word),
        );
        attributes.insert(
            "family".to_string(),
            match segment.kind {
                FinjplKind::Storage => "storage",
                FinjplKind::ProjectFlags => "project-flags",
                FinjplKind::Other => "other",
            }
            .to_string(),
        );
        entries.push(ContainerEntry {
            name: segment
                .name
                .clone()
                .unwrap_or_else(|| format!("FINJPL#{index}")),
            role: role::FINJPL_SEGMENT.to_string(),
            compression: "none".to_string(),
            compressed_size: (segment.range.end - segment.range.start) as u64,
            uncompressed_size: (segment.range.end - segment.range.start) as u64,
            attributes,
        });
    }

    let mut notes = vec![format!(
        "outer V5_CFV2 container: directory offset {} + length {} = {} (file size {}); variant: {}",
        scan.outer_dir_offset,
        scan.outer_dir_length,
        scan.outer_dir_offset as u64 + scan.outer_dir_length as u64,
        scan.data.len(),
        scan.variant.description(),
    )];

    if let Some(dir) = &scan.outer {
        notes.push(format!(
            "outer CATIA_V5 CB0001 directory with {} stream(s)",
            dir.descriptors.len()
        ));
    }

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
    if let Some(version) = &scan.last_save_version {
        notes.push(format!(
            "last saved by CATIA V{}R{} SP{} HF{} ({})",
            version.version,
            version.release,
            version.service_pack,
            version.hot_fix,
            version.build_date
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

#[cfg(test)]
mod tests {
    use super::{identify_variant, Census, InnerDir};
    use crate::variant::Variant;

    #[test]
    fn coherent_e5_stream_overrides_nested_fbb_markers() {
        let inner = InnerDir {
            inner: 0,
            descriptors: Vec::new(),
        };
        let census = Census {
            fbb_runs: 2,
            edge_delimiters: 1,
            ..Census::default()
        };
        assert_eq!(
            identify_variant(Some(&inner), Some(&[]), &census, true),
            Variant::E5Stream
        );
    }
}
