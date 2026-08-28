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

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::bytes::{find, find_from};
use cadmpeg_core::decode::View;
use cadmpeg_core::{ContainerEntry, ContainerSummary};

use crate::layout::extent_struct as extent;
use crate::layout::fbb_face_row as fbb_row;
use crate::layout::inner_header as inner_hdr;
use crate::layout::outer_header as outer_hdr;
use crate::layout::stream_descriptor_header as stream_desc;
use crate::variant::Variant;

/// The outer and inner container magic.
pub const OUTER_MAGIC: &[u8; 8] = &outer_hdr::MAGIC_VALUE;
/// The nested-container stream-directory magic.
pub const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";
/// Marker opening a FINJPL named outer-body segment.
pub const FINJPL_MARKER: &[u8; 8] = &crate::layout::token::NAMED_STREAM_BLOCK;

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

/// One model-container declaration from the outer `Data` logical stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OuterContainerDeclaration {
    /// Byte offset within the reconstructed `Data` stream.
    pub data_offset: usize,
    /// Source ordinal stored by the declaration.
    pub ordinal: u32,
    /// Concrete container class.
    pub class_name: String,
    /// Declared base container class.
    pub base_class: String,
    /// UUID-derived outer stream name selected by the declaration.
    pub stream_name: String,
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
            let type_word = View::u32_be_at(data, pos + FINJPL_MARKER.len())?;
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
    let length = usize::try_from(View::u32_be_at(data, pos + 12)?).ok()?;
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
    let start = find(data, open)? + open.len();
    let relative_end = find(&data[start..], close)?;
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
        let length = usize::from(View::u16_be_at(data, at)?);
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
            let width = View::u16_be_at(data, payload + 3)?;
            let height = View::u16_be_at(data, payload + 1)?;
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
/// The candidate range is the complete preamble or complete FINJPL segment. A
/// candidate must contain at least ten stride-valid records. The preamble wins
/// when coherent; otherwise the segment with the largest valid walk wins, with
/// storage type `0x0000_008e` breaking ties. An unresolved tie rejects E5
/// selection.
#[must_use]
pub fn e5_record_stream(data: &[u8]) -> Option<Range<usize>> {
    let body = outer_body_range(data)?;
    let segments = finjpl_segments(data, body.start, body.end);
    e5_record_stream_in_segments(data, body, &segments)
}

fn outer_body_range(data: &[u8]) -> Option<Range<usize>> {
    data.starts_with(OUTER_MAGIC).then_some(())?;
    let directory_offset =
        usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET)?).ok()?;
    let directory_length =
        usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH)?).ok()?;
    (directory_offset.checked_add(directory_length)? == data.len()
        && directory_length <= directory_offset)
        .then_some(directory_length..directory_offset)
}

/// Return the outer-preamble byte range before the first bounded FINJPL segment.
///
/// The trailing stream directory and every FINJPL segment are outside this
/// range. Zero-entity records and the preferred E5 record stream can use the
/// preamble as an authoritative physical ownership boundary.
/// A zeroed directory pair has no declared directory, so its bytes after the
/// 16-byte prefix form the fallback preamble.
pub(crate) fn outer_preamble_range(data: &[u8]) -> Option<Range<usize>> {
    let body = outer_body_range(data).or_else(|| {
        (data.starts_with(OUTER_MAGIC)
            && View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET) == Some(0)
            && View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH) == Some(0))
        .then_some(outer_hdr::FILL_FF..data.len())
    })?;
    let end = data[body.clone()]
        .windows(FINJPL_MARKER.len())
        .position(|bytes| bytes == FINJPL_MARKER)
        .map_or(body.end, |relative| body.start + relative);
    Some(body.start..end)
}

fn e5_record_stream_in_segments(
    data: &[u8],
    body: Range<usize>,
    segments: &[FinjplSegment],
) -> Option<Range<usize>> {
    let preamble = outer_preamble_range(data)?;
    if coherent_e5_record_count(&data[preamble.clone()]) >= 10 {
        return Some(preamble);
    }

    let candidates = segments
        .iter()
        .filter(|segment| segment.range.start >= body.start && segment.range.end <= body.end)
        .filter_map(|segment| {
            let count = coherent_e5_record_count(&data[segment.range.clone()]);
            (count >= 10).then_some((
                count,
                segment.type_word == 0x0000_008e,
                segment.range.clone(),
            ))
        })
        .collect::<Vec<_>>();
    let best_count = candidates.iter().map(|(count, _, _)| *count).max()?;
    let mut best = candidates
        .into_iter()
        .filter(|(count, _, _)| *count == best_count)
        .collect::<Vec<_>>();
    let preferred = best.iter().any(|(_, preferred, _)| *preferred);
    if preferred {
        best.retain(|(_, preferred, _)| *preferred);
    }
    (best.len() == 1).then(|| best.pop().expect("one candidate remains").2)
}

fn coherent_e5_record_count(data: &[u8]) -> usize {
    e5_record_spans(data).len()
}

/// Return the longest declared-stride E5 walk in a bounded byte region.
///
/// A stream may place the unframed `05 08 01` coordinate roster between E5
/// records. No other gap is part of the walk: accepting arbitrary bytes here
/// would turn an incidental marker into a record and could select the wrong
/// family before the route decoder sees the data.
pub(crate) fn e5_record_spans(data: &[u8]) -> Vec<Range<usize>> {
    let mut best = Vec::new();
    let mut search = 0;
    while search < data.len() {
        let Some(relative) = data[search..]
            .windows(E5_MARKER.len())
            .position(|bytes| bytes == E5_MARKER)
        else {
            break;
        };
        let start = search + relative;
        let (walk, consumed) = e5_record_walk(data, start);
        if walk.len() > best.len() {
            best = walk;
        }
        search = consumed.max(start.saturating_add(1));
    }
    best
}

/// Return every valid `E5 0D 03` frame in a bounded stream region.
///
/// The complete E5 carrier stream may interleave these frames with other
/// framed E5 records. Route selection still uses [`e5_record_spans`] because
/// it needs a contiguous declared-stride walk; carrier decoders need the
/// complete frame inventory instead.
pub(crate) fn all_e5_record_spans(data: &[u8]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut search = 0;
    while search < data.len() {
        let Some(relative) = data[search..]
            .windows(E5_MARKER.len())
            .position(|bytes| bytes == E5_MARKER)
        else {
            break;
        };
        let start = search + relative;
        let Some(end) = e5_record_end(data, start) else {
            search = start.saturating_add(1);
            continue;
        };
        spans.push(start..end);
        search = end;
    }
    spans
}

fn e5_record_walk(data: &[u8], start: usize) -> (Vec<Range<usize>>, usize) {
    let mut walk = Vec::new();
    let mut position = start;
    let mut consumed = start;
    while let Some(end) = e5_record_end(data, position) {
        walk.push(position..end);
        position = end;
        consumed = end;

        if e5_marker_at(data, position) {
            continue;
        }
        let Some(next) = skip_e5_vertex_rows(data, position) else {
            break;
        };
        consumed = next;
        if !e5_marker_at(data, next) {
            break;
        }
        position = next;
    }
    (walk, consumed)
}

fn e5_record_end(data: &[u8], position: usize) -> Option<usize> {
    if !e5_marker_at(data, position) {
        return None;
    }
    let header = data.get(position..position.checked_add(7)?)?;
    let size = usize::from(View::u16_le_at(header, 5)?);
    let end = position.checked_add(size.checked_add(13)?)?;
    (end <= data.len()).then_some(end)
}

fn skip_e5_vertex_rows(data: &[u8], mut position: usize) -> Option<usize> {
    let start = position;
    while vertex_row_at(data, position) {
        position = position.checked_add(15)?;
    }
    (position != start).then_some(position)
}

fn e5_marker_at(data: &[u8], position: usize) -> bool {
    let Some(end) = position.checked_add(E5_MARKER.len()) else {
        return false;
    };
    data.get(position..end) == Some(E5_MARKER.as_slice())
}

fn vertex_row_at(data: &[u8], position: usize) -> bool {
    let Some(row_end) = position.checked_add(15) else {
        return false;
    };
    data.get(position..row_end)
        .is_some_and(|row| row[..3] == [0x05, 0x08, 0x01])
}

/// Standard-nested BREP-spine markers used for variant identification.
const EDGE_DELIMITER: &[u8; 8] = &[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_MARKER: &[u8; 3] = &[0x05, 0x08, 0x01];
pub(crate) const E5_MARKER: &[u8; 3] = &[0xe5, 0x0d, 0x03];

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
    /// Raw trailing extent flags word.
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
    /// Contiguous stride-8 FBB runs in the BREP stream.
    pub fbb_runs: usize,
    /// Stride-8 FBB face rows in the BREP stream.
    pub fbb_face_rows: usize,
    /// `10 24 04 ff ff 00 00 00` standard edge-table delimiters in the BREP stream.
    pub edge_delimiters: usize,
    /// `05 08 01` vertex-record signatures in the BREP stream.
    pub vertex_markers: usize,
    /// Complete `a9 03` records in the outer preamble.
    pub a9_records: usize,
    /// `e5 0d 03` record-family markers in the outer body.
    pub e5_markers: usize,
}

/// Everything read from a `.CATPart`, shared by `inspect` and `decode`.
pub struct ContainerScan<'a> {
    /// The whole file image.
    pub data: Cow<'a, [u8]>,
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
    /// Reconstructed canonical `MainDataStream`, which owns the standard FBB spine.
    pub main_data_stream: Option<Vec<u8>>,
    /// Exact JPEG previews extracted from summary-information framing.
    pub previews: Vec<PreviewImage>,
    /// Unique saved-by application version from summary information.
    pub last_save_version: Option<LastSaveVersion>,
    /// External CATIA documents named by storage properties.
    pub external_references: Vec<ExternalReference>,
    /// Every bounded outer FINJPL block in source order.
    pub finjpl_segments: Vec<FinjplSegment>,
    /// Exact model-container declarations from the outer `Data` stream.
    pub outer_container_declarations: Vec<OuterContainerDeclaration>,
    /// Canonical outer persistent-surface aliases available to geometry routes.
    pub(crate) surface_alias_tags: HashMap<u32, Option<u32>>,
    /// Record-family census.
    pub census: Census,
    /// Identified storage variant.
    pub variant: Variant,
}

/// Return the logical record sources that can carry consolidated A/B records.
///
/// Each catalogued descriptor is one source. Its physical extents remain in
/// logical-offset order. When a
/// file has no outer directory, the bytes after the outer header and before a
/// nested container (or the outer directory) are the unnamed outer-preamble
/// source. The nested directory itself and all directory headers stay outside
/// the inventory. Records can establish ordered relationships across extents
/// of one descriptor, but never across descriptors.
pub(crate) fn consolidated_record_sources(scan: &ContainerScan<'_>) -> Vec<Vec<Range<usize>>> {
    let mut sources = Vec::new();
    let add_directory = |sources: &mut Vec<Vec<Range<usize>>>, directory: &InnerDir| {
        for descriptor in &directory.descriptors {
            let mut source = Vec::new();
            for extent in &descriptor.extents {
                let Some(start) = directory.inner.checked_add(extent.phys_off as usize) else {
                    continue;
                };
                let Some(end) = start.checked_add(extent.phys_len as usize) else {
                    continue;
                };
                if end <= scan.data.len() {
                    source.push(start..end);
                }
            }
            if !source.is_empty() && !sources.contains(&source) {
                sources.push(source);
            }
        }
    };

    if let Some(outer) = scan.outer.as_ref() {
        add_directory(&mut sources, outer);
    } else {
        let outer_end = scan
            .inner
            .as_ref()
            .map(|directory| directory.inner)
            .or_else(|| outer_stream_directory_range(&scan.data).map(|range| range.start))
            .unwrap_or(scan.data.len());
        if outer_hdr::FILL_FF < outer_end {
            sources.push(std::iter::once(outer_hdr::FILL_FF..outer_end).collect());
        }
    }
    if let Some(inner) = scan.inner.as_ref() {
        add_directory(&mut sources, inner);
    }

    if sources.is_empty() && scan.data.len() > outer_hdr::FILL_FF {
        sources.push(std::iter::once(outer_hdr::FILL_FF..scan.data.len()).collect());
    }
    sources
}

/// Flatten the descriptor-scoped source inventory without changing logical
/// source or extent order.
#[cfg(test)]
pub(crate) fn consolidated_record_ranges(scan: &ContainerScan<'_>) -> Vec<Range<usize>> {
    consolidated_record_sources(scan)
        .into_iter()
        .flatten()
        .collect()
}

/// Reconstruct each catalogued logical stream as an independent record source.
///
/// Records cannot establish adjacency or one object-id namespace across two
/// descriptors. A container without a parsed directory has one unnamed source:
/// its bounded outer preamble.
pub(crate) fn logical_record_streams(scan: &ContainerScan<'_>) -> Vec<Vec<u8>> {
    let mut streams = [scan.outer.as_ref(), scan.inner.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|directory| {
            directory.descriptors.iter().filter_map(|descriptor| {
                let stream = reconstruct_logical_stream(&scan.data, descriptor, directory.inner);
                (!stream.is_empty()).then_some(stream)
            })
        })
        .collect::<Vec<_>>();
    if streams.is_empty() {
        if let Some(range) = outer_preamble_range(&scan.data) {
            streams.push(scan.data[range].to_vec());
        }
    }
    streams
}

/// Whether a byte prefix is a `.CATPart`: the `V5_CFV2\0` outer magic is unique
/// to Dassault's container and is a conclusive signal on its own.
pub fn looks_like_catia(prefix: &[u8]) -> bool {
    prefix.starts_with(OUTER_MAGIC)
}

/// Return maximal contiguous stride-8 FBB groups in source order.
pub(crate) fn fbb_run_ranges(body: &[u8]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut position = 0;
    while position + fbb_row::LEN <= body.len() {
        if is_fbb_row(&body[position..]) {
            let start = position;
            while position + fbb_row::LEN <= body.len() && is_fbb_row(&body[position..]) {
                position += fbb_row::LEN;
            }
            ranges.push(start..position);
        } else {
            position += 1;
        }
    }
    ranges
}

/// A standard face-outer-bound row. Bit 7 of the leading `30` byte is a form
/// flag; the structural `04 04 ff` tail is stable.
pub(crate) fn is_fbb_row(bytes: &[u8]) -> bool {
    bytes.len() >= fbb_row::ALPHA
        && bytes[0] & 0x7f == 0x30
        && bytes[1..fbb_row::ALPHA] == [0x04, 0x04, 0xff]
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
    if data.len() < inner_hdr::LEN {
        return None;
    }
    let inner = find_from(data, OUTER_MAGIC, OUTER_MAGIC.len())?;
    let a = View::u32_be_at(data, inner.checked_add(inner_hdr::DIRECTORY_OFFSET_DELTA)?)? as usize;
    let b = View::u32_be_at(data, inner.checked_add(inner_hdr::DIRECTORY_LENGTH)?)?;
    let dir_offset = inner.checked_add(a)?;
    let magic_end = dir_offset.checked_add(DIR_MAGIC.len())?;
    if data.get(dir_offset..magic_end) != Some(DIR_MAGIC) {
        return None;
    }
    let b_usize = b as usize;
    if b == 0 || dir_offset.checked_add(b_usize)? > data.len() {
        return None;
    }
    parse_directory_region(data, inner, dir_offset, b_usize)
}

/// Parse the outer `CATIA_V5 CB0001` stream directory. Physical extent offsets
/// in its descriptors are absolute file offsets.
#[must_use]
pub fn parse_outer_stream_directory(data: &[u8]) -> Option<InnerDir> {
    parse_outer_stream_directory_with_range(data).map(|(_, directory)| directory)
}

/// Parse and return the exact outer stream-directory byte range.
#[must_use]
pub fn outer_stream_directory_range(data: &[u8]) -> Option<Range<usize>> {
    parse_outer_stream_directory_with_range(data).map(|(range, _)| range)
}

fn parse_outer_stream_directory_with_range(data: &[u8]) -> Option<(Range<usize>, InnerDir)> {
    let dir_offset = usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET)?).ok()?;
    let dir_length = usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH)?).ok()?;
    let dir_end = dir_offset.checked_add(dir_length)?;
    (dir_end == data.len()).then_some(())?;
    let directory = parse_directory_region(data, 0, dir_offset, dir_length)?;
    Some((dir_offset..dir_end, directory))
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
    // real descriptor. The extent count sits at `desc_offset + EXTENT_COUNT`.
    let mut o = 0usize;
    while o + 4 <= dirbuf.len() {
        let Some(k) = View::u32_be_at(dirbuf, o).and_then(|value| usize::try_from(value).ok())
        else {
            break;
        };
        let extents_end = k
            .checked_mul(extent::LEN)
            .and_then(|extent_bytes| o.checked_add(4)?.checked_add(extent_bytes));
        if k != 0 && extents_end.is_some_and(|end| end <= dirbuf.len()) {
            if let Some((extents, cum)) = parse_extents(dirbuf, o, k, physical_base, file_len) {
                if cum > 0 && o >= stream_desc::EXTENT_COUNT {
                    let ds = o - stream_desc::EXTENT_COUNT;
                    let logical_length =
                        View::u32_be_at(dirbuf, ds + stream_desc::LOGICAL_STREAM_LENGTH)
                            .unwrap_or(0);
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
        let base = o + 4 + extent::LEN * i;
        let phys_off = View::u32_be_at(dirbuf, base + extent::PHYS_OFF)?;
        let phys_len = View::u32_be_at(dirbuf, base + extent::PHYS_LEN)?;
        let log_len = View::u32_be_at(dirbuf, base + extent::LOG_LEN)?;
        let log_off = View::u32_be_at(dirbuf, base + extent::LOG_OFF)?;
        let flags = View::u32_be_at(dirbuf, base + extent::FLAGS)?;
        let phys_end = physical_base
            .checked_add(phys_off as usize)?
            .checked_add(phys_len as usize)?;
        if phys_len == 0 || phys_end > file_len || log_off as usize != cum || log_len != phys_len {
            return None;
        }
        cum = cum.checked_add(log_len as usize)?;
        extents.push(Extent {
            phys_off,
            phys_len,
            flags,
        });
    }
    Some((extents, cum))
}

/// Read a descriptor's UTF-16LE ASCII stream name from one of its two framed
/// name locations.
///
/// The descriptor tail is a two-byte UTF-16LE terminator followed by one zero
/// padding byte. The name is the complete run of printable ASCII code units
/// immediately before that tail. This end anchor keeps unrelated UTF-16 text
/// elsewhere in the descriptor from becoming the stream name.
fn descriptor_name(dirbuf: &[u8], ds: usize) -> String {
    if let Some(tail_start) = ds.checked_sub(3) {
        if dirbuf.get(tail_start..ds) == Some(&[0, 0, 0]) {
            let mut name_start = tail_start;
            while name_start >= 2 {
                let pair_start = name_start - 2;
                if (0x20..0x7f).contains(&dirbuf[pair_start]) && dirbuf[pair_start + 1] == 0 {
                    name_start = pair_start;
                } else {
                    break;
                }
            }
            let name_bytes = &dirbuf[name_start..tail_start];
            if name_bytes.len() >= 6 {
                return name_bytes
                    .chunks_exact(2)
                    .map(|pair| pair[0] as char)
                    .collect();
            }
        }
    }

    // Older directory headers place an unframed name at ds+0x10. Admit this
    // form only when the name closes with a UTF-16LE terminator and the rest
    // of the header before the extent count is zero.
    let Some(header_name_start) = ds.checked_add(0x10) else {
        return String::new();
    };
    let Some(header_end) = ds.checked_add(stream_desc::EXTENT_COUNT) else {
        return String::new();
    };
    let Some(header_name) = dirbuf.get(header_name_start..header_end) else {
        return String::new();
    };
    let mut name_len = 0;
    while name_len + 1 < header_name.len()
        && (0x20..0x7f).contains(&header_name[name_len])
        && header_name[name_len + 1] == 0
    {
        name_len += 2;
    }
    if name_len < 6
        || header_name.get(name_len..name_len + 2) != Some(&[0, 0])
        || header_name
            .get(name_len + 2..)
            .is_none_or(|rest| rest.iter().any(|byte| *byte != 0))
    {
        return String::new();
    }

    header_name[..name_len]
        .chunks_exact(2)
        .map(|pair| pair[0] as char)
        .collect()
}

/// Concatenate a logical stream's physical extents in `log_off` order.
pub fn reconstruct_logical_stream(data: &[u8], descriptor: &Descriptor, inner: usize) -> Vec<u8> {
    let Some(logical_length) =
        descriptor
            .extents
            .iter()
            .try_fold(0usize, |logical_length, extent| {
                let start = inner.checked_add(extent.phys_off as usize)?;
                let end = start.checked_add(extent.phys_len as usize)?;
                (end <= data.len())
                    .then(|| logical_length.checked_add(end - start))
                    .flatten()
            })
    else {
        return Vec::new();
    };
    if logical_length != descriptor.logical_length as usize {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(logical_length);
    for extent in &descriptor.extents {
        let start = inner + extent.phys_off as usize;
        let end = start + extent.phys_len as usize;
        out.extend_from_slice(&data[start..end]);
    }
    out
}

/// Decode model-container declarations whose UUIDs select named outer streams.
#[must_use]
pub fn outer_container_declarations(
    data: &[u8],
    outer: &InnerDir,
) -> Vec<OuterContainerDeclaration> {
    let mut data_descriptors = outer
        .descriptors
        .iter()
        .filter(|descriptor| descriptor.name == "Data");
    let Some(data_descriptor) = data_descriptors.next() else {
        return Vec::new();
    };
    if data_descriptors.next().is_some() {
        return Vec::new();
    }
    let stream_names = outer
        .descriptors
        .iter()
        .map(|descriptor| descriptor.name.as_str())
        .collect::<HashSet<_>>();
    let logical = reconstruct_logical_stream(data, data_descriptor, outer.inner);
    parse_outer_container_declarations(&logical, &stream_names)
}

/// Select the unique declared outer container whose physical extent contains
/// the complete file range.
#[must_use]
pub fn outer_container_for_extent<'a>(
    outer: &InnerDir,
    declarations: &'a [OuterContainerDeclaration],
    byte_offset: u64,
    byte_len: u64,
) -> Option<&'a OuterContainerDeclaration> {
    let byte_end = byte_offset.checked_add(byte_len)?;
    let physical_base = u64::try_from(outer.inner).ok()?;
    let mut containing = declarations.iter().filter(|declaration| {
        outer
            .descriptors
            .iter()
            .filter(|descriptor| descriptor.name == declaration.stream_name)
            .flat_map(|descriptor| &descriptor.extents)
            .any(|extent| {
                let extent_start = u64::from(extent.phys_off).checked_add(physical_base);
                extent_start.is_some_and(|extent_start| {
                    extent_start <= byte_offset
                        && extent_start
                            .checked_add(u64::from(extent.phys_len))
                            .is_some_and(|extent_end| byte_end <= extent_end)
                })
            })
    });
    let declaration = containing.next()?;
    containing.next().is_none().then_some(declaration)
}

fn parse_outer_container_declarations(
    data: &[u8],
    stream_names: &HashSet<&str>,
) -> Vec<OuterContainerDeclaration> {
    const HEADER: &[u8] = b"\x01\x00\x03\x00";
    const PREFIX: &[u8] = b"\x01\x00\x6c\x00\x02\x00\x00\x00";
    const CLASS_BLOCK: &[u8] = b"\x02\x00\x81\x20";
    const TERMINAL: &[u8] = b"\x03\x00\xf7\x00\x03\x00\x00\x00";

    let mut declarations = Vec::new();
    for start in 0..data.len().saturating_sub(64) {
        if data.get(start + 8..start + 12) != Some(HEADER)
            || data.get(start + 16..start + 24) != Some(PREFIX)
            || data.get(start + 32..start + 36) != Some(CLASS_BLOCK)
        {
            continue;
        }
        let strings_start = start + 40;
        let Some(relative_terminal) = memchr::memmem::find(&data[strings_start..], TERMINAL) else {
            continue;
        };
        let terminal = strings_start + relative_terminal;
        let Some((class_name, base_class)) = declaration_class_pair(&data[strings_start..terminal])
        else {
            continue;
        };
        let Some(uuid) = data.get(terminal + TERMINAL.len()..terminal + TERMINAL.len() + 16) else {
            continue;
        };
        let Some((first, middle, last)) = View::u32_be_at(uuid, 4)
            .zip(View::u32_be_at(uuid, 8))
            .zip(View::u32_be_at(uuid, 12))
            .map(|((first, middle), last)| (first, middle, last))
        else {
            continue;
        };
        let canonical_stream_name = format!("{first:x}_{middle:08x}_{last:x}");
        let prefixed_stream_name = format!("_{canonical_stream_name}");
        let stream_name = match (
            stream_names.contains(canonical_stream_name.as_str()),
            stream_names.contains(prefixed_stream_name.as_str()),
        ) {
            (true, false) => canonical_stream_name,
            (false, true) => prefixed_stream_name,
            (false, false) | (true, true) => continue,
        };
        let Some(ordinal) = View::u32_le_at(data, start + 12) else {
            continue;
        };
        declarations.push(OuterContainerDeclaration {
            data_offset: start,
            ordinal,
            class_name,
            base_class,
            stream_name,
        });
    }
    let mut selected_streams = HashSet::new();
    if declarations
        .iter()
        .any(|declaration| !selected_streams.insert(declaration.stream_name.as_str()))
    {
        return Vec::new();
    }
    declarations
}

fn declaration_class_pair(data: &[u8]) -> Option<(String, String)> {
    let first_end = data.iter().position(|byte| *byte == 0)?;
    let second_start = first_end.checked_add(1)?;
    let second_end = second_start.checked_add(
        data.get(second_start..)?
            .iter()
            .position(|byte| *byte == 0)?,
    )?;
    let first = data.get(..first_end)?;
    let second = data.get(second_start..second_end)?;
    if first.is_empty()
        || second.is_empty()
        || data.get(second_end..)?.iter().any(|byte| *byte != 0)
        || !first.iter().chain(second).all(u8::is_ascii_graphic)
    {
        return None;
    }
    Some((
        std::str::from_utf8(first).ok()?.to_owned(),
        std::str::from_utf8(second).ok()?.to_owned(),
    ))
}

/// Reconstruct the logical BREP buffer: the uniquely largest canonical
/// `MainDataStream` followed by the uniquely largest canonical `SurfacicReps`
/// ([spec §3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)). Both are required. A directory that
/// catalogues the BREP body carries both canonical streams; the contiguous-body
/// exception has neither and returns `None`.
pub fn brep_stream(data: &[u8], dir: &InnerDir) -> Option<Vec<u8>> {
    let mut out = main_data_stream(data, dir)?;
    let surf = unique_largest_descriptor(
        dir.descriptors
            .iter()
            .filter(|descriptor| descriptor.name == "SurfacicReps"),
    )?;
    out.extend(reconstruct_logical_stream(data, surf, dir.inner));
    Some(out)
}

/// Reconstruct the unique canonical `MainDataStream`, which owns the FBB
/// topology spine. The surface stream is deliberately excluded: its numeric
/// payload may contain byte sequences that resemble FBB rows but cannot assign
/// topology faces.
pub(crate) fn main_data_stream(data: &[u8], dir: &InnerDir) -> Option<Vec<u8>> {
    let main = unique_largest_descriptor(
        dir.descriptors
            .iter()
            .filter(|descriptor| descriptor.name == "MainDataStream"),
    )?;
    Some(reconstruct_logical_stream(data, main, dir.inner))
}

fn unique_largest_descriptor<'a>(
    descriptors: impl IntoIterator<Item = &'a Descriptor>,
) -> Option<&'a Descriptor> {
    let mut selected = None;
    let mut selected_length = 0;
    let mut equal_count = 0;
    for descriptor in descriptors {
        if selected.is_none() || descriptor.logical_length > selected_length {
            selected = Some(descriptor);
            selected_length = descriptor.logical_length;
            equal_count = 1;
        } else if descriptor.logical_length == selected_length {
            equal_count += 1;
        }
    }
    (equal_count == 1).then_some(selected).flatten()
}

/// Identify the storage variant from container-level evidence ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#1-variant-families)).
///
/// The identification is intentionally structural: standard-nested requires an
/// FBB spine plus an admitted standard edge-table grammar; FBB-only requires
/// an admitted two-table FBB edge grammar. The delimiter byte sequence is not
/// sufficient to distinguish them because FBB-only widths one and three reuse
/// the standard delimiter. Zero-entity requires no nested container and an
/// `a9 03` family; the object-stream / E5 families are named from their record
/// census. An admitted standard edge-table grammar is a complete nested FBB
/// spine and owns route selection over a coherent E5 walk. An FBB-only grammar
/// is a partial spine; when it coexists with a coherent E5 walk, E5 owns the
/// route. Anything that matches no invariant is [`Variant::Unknown`].
fn identify_variant(
    inner: Option<&InnerDir>,
    brep: Option<&[u8]>,
    main_data_stream: Option<&[u8]>,
    census: &Census,
    coherent_e5: bool,
) -> Variant {
    match (inner, brep) {
        // A standard edge table establishes a complete nested FBB body. It
        // takes precedence over an unrelated E5 stream in the same container.
        (Some(_), Some(brep)) if census.fbb_runs > 0 => {
            let variant = identify_fbb_variant(main_data_stream.unwrap_or(brep), census);
            if variant == Variant::FbbOnly && coherent_e5 {
                Variant::E5Stream
            } else {
                variant
            }
        }
        // E5 is the geometry route when no complete nested FBB body is present.
        _ if coherent_e5 => Variant::E5Stream,
        // No nested container at all.
        (None, _) => {
            if census.a9_records > 0 {
                Variant::ZeroEntity
            } else {
                Variant::Unknown
            }
        }
        // Nested container, but its directory catalogues no BREP body.
        (Some(_), None) => Variant::InnerNoDirectory,
        (Some(_), Some(_)) => Variant::FloatPackedInnerNoFbb,
    }
}

fn identify_fbb_variant(brep: &[u8], census: &Census) -> Variant {
    if crate::families::standard::fbb::standard_edge_count(brep).is_some() {
        return Variant::StandardNested;
    }
    if crate::families::standard::fbb::fbb_only_edge_count(brep).is_some() {
        return Variant::FbbOnly;
    }
    if census.edge_delimiters == 0 && census.vertex_markers > 0 {
        Variant::FbbOnly
    } else {
        Variant::Unknown
    }
}

/// Identify a whole `.CATPart` byte image.
pub fn scan_bytes<'a>(data: impl Into<Cow<'a, [u8]>>) -> ContainerScan<'a> {
    let data = data.into();
    let outer_dir_offset = View::u32_be_at(&data, outer_hdr::DIRECTORY_OFFSET).unwrap_or(0);
    let outer_dir_length = View::u32_be_at(&data, outer_hdr::DIRECTORY_LENGTH).unwrap_or(0);

    let outer = parse_outer_stream_directory(&data);
    let inner = parse_stream_directory(&data);
    let brep = inner.as_ref().and_then(|dir| brep_stream(&data, dir));
    let main_data_stream = inner.as_ref().and_then(|dir| main_data_stream(&data, dir));
    let outer_body = outer_body_range(&data);
    let finjpl_segments = outer_body.as_ref().map_or_else(Vec::new, |body| {
        finjpl_segments(&data, body.start, body.end)
    });
    let previews = preview_images_in_segments(&data, &finjpl_segments);
    let last_save_version = last_save_version_in_segments(&data, &finjpl_segments);
    let external_references = external_references_in_segments(&data, &finjpl_segments);
    let outer_container_declarations = outer.as_ref().map_or_else(Vec::new, |directory| {
        outer_container_declarations(&data, directory)
    });

    let mut census = Census {
        a9_records: outer_preamble_range(&data).map_or(0, |range| {
            crate::families::zero_entity::records::zero_entity_record_inventory_in_range(
                &data, range,
            )
            .len()
        }),
        e5_markers: outer_body
            .as_ref()
            .map_or(0, |body| count_subslice(&data[body.clone()], E5_MARKER)),
        ..Default::default()
    };
    if let Some(b) = main_data_stream.as_deref() {
        let fbb_ranges = fbb_run_ranges(b);
        census.fbb_runs = fbb_ranges.len();
        census.fbb_face_rows = fbb_ranges
            .iter()
            .map(|range| (range.end - range.start) / fbb_row::LEN)
            .sum();
        census.edge_delimiters = count_subslice(b, EDGE_DELIMITER);
        census.vertex_markers = count_subslice(b, VERTEX_MARKER);
    }

    let variant = identify_variant(
        inner.as_ref(),
        brep.as_deref(),
        main_data_stream.as_deref(),
        &census,
        outer_body.is_some_and(|body| {
            e5_record_stream_in_segments(&data, body, &finjpl_segments).is_some()
        }),
    );
    let surface_alias_tags = matches!(variant, Variant::StandardNested)
        .then(|| crate::object_graph::surface_alias_tag_map(&data))
        .unwrap_or_default();

    ContainerScan {
        data,
        outer_dir_offset,
        outer_dir_length,
        outer,
        inner,
        brep,
        main_data_stream,
        previews,
        last_save_version,
        external_references,
        finjpl_segments,
        outer_container_declarations,
        surface_alias_tags,
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
            attributes.insert(
                "extent_flags".to_string(),
                d.extents
                    .iter()
                    .map(|extent| format!("0x{:08x}", extent.flags))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            if directory == "outer" {
                if let Some(declaration) = scan
                    .outer_container_declarations
                    .iter()
                    .find(|declaration| declaration.stream_name == d.name)
                {
                    attributes.insert(
                        "container_class".to_string(),
                        declaration.class_name.clone(),
                    );
                    attributes.insert(
                        "container_base_class".to_string(),
                        declaration.base_class.clone(),
                    );
                    attributes.insert(
                        "container_ordinal".to_string(),
                        declaration.ordinal.to_string(),
                    );
                    attributes.insert(
                        "container_data_offset".to_string(),
                        declaration.data_offset.to_string(),
                    );
                }
            }
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
            "reconstructed BREP stream from MainDataStream + SurfacicReps: {} FBB group(s) \
             containing {} face row(s), {} vertex record(s), {} edge-table delimiter(s)",
            scan.census.fbb_runs,
            scan.census.fbb_face_rows,
            scan.census.vertex_markers,
            scan.census.edge_delimiters
        ));
    }
    if scan.census.a9_records > 0 || scan.census.e5_markers > 0 {
        notes.push(format!(
            "record-family census: {} a9 03, {} e5 0d 03",
            scan.census.a9_records, scan.census.e5_markers
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

    let dialects = vec![crate::dialect::classify(scan)];
    ContainerSummary {
        dialects,
        format: crate::dialect::FORMAT.to_string(),
        container_kind: "v5-cfv2".to_string(),
        entries,
        notes,
    }
}

#[cfg(test)]
mod tests;
