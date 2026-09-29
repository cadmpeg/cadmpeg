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

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::ops::Range;

use cadmpeg_core::bytes::{find, find_from};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::layout::extent_struct as extent;
use crate::layout::fbb_face_row as fbb_row;
use crate::layout::inner_header as inner_hdr;
use crate::layout::outer_header as outer_hdr;
use crate::layout::stream_descriptor_header as stream_desc;
use crate::variant::Variant;
use crate::wire::records::SourceExtent;

/// The outer and inner container magic.
pub(crate) const OUTER_MAGIC: &[u8; 8] = &outer_hdr::MAGIC_VALUE;
/// The nested-container stream-directory magic.
pub(crate) const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";
/// Marker opening a FINJPL named outer-body segment.
const FINJPL_MARKER: &[u8; 8] = &crate::layout::token::NAMED_STREAM_BLOCK;

/// Semantic family of a FINJPL segment's big-endian type word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinjplKind {
    /// `CATStorageProperty` carrier.
    Storage,
    /// `CATProjectFlags` or `CATSummaryInformation` carrier.
    ProjectFlags,
    /// Manufacturer, OSMX, preview, or other named block.
    Other,
}

/// One FINJPL segment bounded by the next marker or the supplied body end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinjplSegment {
    /// Complete byte range beginning at the marker.
    pub(crate) range: Range<usize>,
    /// Big-endian type word immediately following the marker.
    pub(crate) type_word: u32,
    /// Primary length-prefixed ASCII block name, when present.
    pub(crate) name: Option<String>,
}

impl FinjplSegment {
    /// Classified type family.
    pub(crate) fn kind(&self) -> FinjplKind {
        match self.type_word {
            0x0000_0080 | 0x0000_0082 | 0x0000_0084 | 0x0000_0086 | 0x0000_008e | 0x0000_0090
            | 0x0000_0092 => FinjplKind::Storage,
            0x0101_0001..=0x0101_0003 => FinjplKind::ProjectFlags,
            _ => FinjplKind::Other,
        }
    }
}

/// One complete JPEG preview embedded in a summary-information segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewImage {
    /// Exact file range from JPEG SOI through EOI.
    pub(crate) range: Range<usize>,
    /// Pixel width from the JPEG start-of-frame segment.
    pub(crate) width: u16,
    /// Pixel height from the JPEG start-of-frame segment.
    pub(crate) height: u16,
    /// Component count from the JPEG start-of-frame segment.
    pub(crate) components: u8,
}

/// CATIA application version stored by the summary-information record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LastSaveVersion {
    /// CATIA generation number.
    pub(crate) version: u16,
    /// CATIA release number.
    pub(crate) release: u16,
    /// Installed service-pack number.
    pub(crate) service_pack: u16,
    /// Installed hot-fix number.
    pub(crate) hot_fix: u16,
    /// Source build-date string.
    pub(crate) build_date: String,
}

/// One external CATIA document named by a storage-property record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalReference {
    /// File offset of the length-prefixed target string.
    pub(crate) offset: usize,
    /// Referenced CATIA document name or path.
    pub(crate) target: String,
}

/// One model-container declaration from the outer `Data` logical stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OuterContainerDeclaration {
    /// Byte offset within the reconstructed `Data` stream.
    pub(crate) data_offset: usize,
    /// Source ordinal stored by the declaration.
    pub(crate) ordinal: u32,
    /// Concrete container class.
    pub(crate) class_name: String,
    /// Declared base container class.
    pub(crate) base_class: String,
    /// UUID-derived outer stream name selected by the declaration.
    pub(crate) stream_name: String,
}

/// A body extent proved to lie inside the container image it indexes.
///
/// The outer directory declares where the body ends. A declared end past the
/// end of the image is an inconsistency in bytes that are present, and no
/// constructor of this type admits one: the type cannot state an overrun, so a
/// scan over it never reads a silently shortened region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyExtent<'a> {
    image: &'a [u8],
    range: Range<usize>,
}

impl<'a> BodyExtent<'a> {
    /// The complete image.
    #[must_use]
    pub(crate) fn whole(image: &'a [u8]) -> Self {
        Self {
            range: 0..image.len(),
            image,
        }
    }

    /// The body between a declared outer-directory length and offset.
    ///
    /// `offset + length == image.len()` with `length <= offset` proves
    /// `offset <= image.len()`, so an admitted pair states an extent the image
    /// holds. Any other pair declares no outer body at all.
    fn from_directory_pair(image: &'a [u8], offset: usize, length: usize) -> Option<Self> {
        (offset.checked_add(length)? == image.len() && length <= offset).then_some(Self {
            image,
            range: length..offset,
        })
    }

    /// The image from `start` through its end.
    fn tail_from(image: &'a [u8], start: usize) -> Option<Self> {
        (start <= image.len()).then_some(Self {
            image,
            range: start..image.len(),
        })
    }

    /// The proved byte range within the image.
    #[must_use]
    fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// The bytes of the extent.
    #[must_use]
    fn bytes(&self) -> &'a [u8] {
        &self.image[self.range.start..self.range.end]
    }
}

/// Split FINJPL segments within a bounded outer-body extent.
pub(crate) fn finjpl_segments(
    ctx: &DecodeContext<'_>,
    body: &BodyExtent<'_>,
) -> Result<Vec<FinjplSegment>, CodecError> {
    let data = body.image;
    let body_start = body.range.start;
    let end = body.range.end;
    if body_start >= end {
        return Ok(Vec::new());
    }
    let positions = ctx.collect_vec(
        memchr::memmem::find_iter(&data[body_start..end], FINJPL_MARKER)
            .map(|relative| body_start + relative),
        "catia_finjpl_positions",
    )?;
    let mut segments = Vec::new();
    for (index, &pos) in positions.iter().enumerate() {
        let Some(type_word) = View::u32_be_at(data, pos + FINJPL_MARKER.len()) else {
            continue;
        };
        let segment_end = positions.get(index + 1).copied().unwrap_or(end);
        let name = finjpl_primary_name(ctx, data, pos, segment_end)?;
        ctx.push_vec(
            &mut segments,
            FinjplSegment {
                range: pos..segment_end,
                type_word,
                name,
            },
            "catia_finjpl_segments",
        )?;
    }
    Ok(segments)
}

fn finjpl_primary_name(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    pos: usize,
    end: usize,
) -> Result<Option<String>, CodecError> {
    let Some(length) =
        View::u32_be_at(data, pos + 12).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    let Some(start) = pos.checked_add(17) else {
        return Ok(None);
    };
    let Some(name_end) = start.checked_add(length) else {
        return Ok(None);
    };
    if data.get(pos + 16) != Some(&0) || name_end > end {
        return Ok(None);
    }
    let Some(value) = data.get(start..name_end) else {
        return Ok(None);
    };
    if value.is_empty() || !value.iter().all(|byte| matches!(byte, 0x20..=0x7e)) {
        return Ok(None);
    }
    let Some(value) = std::str::from_utf8(value).ok() else {
        return Ok(None);
    };
    Ok(Some(ctx.copy_retained_text(value, "catia_finjpl_name")?))
}

/// Extract length-closed JPEG previews from `CATSummaryInformation` FINJPL
/// segments. JPEG marker framing supplies both dimensions and the exact image
/// boundary; incidental JPEG signatures outside this segment family are ignored.
pub(crate) fn preview_images(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<PreviewImage>, CodecError> {
    let segments = finjpl_segments(ctx, &BodyExtent::whole(data))?;
    preview_images_in_segments(ctx, data, &segments)
}

fn preview_images_in_segments(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    segments: &[FinjplSegment],
) -> Result<Vec<PreviewImage>, CodecError> {
    ctx.collect_vec(
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
                let (relative_start, relative_end, width, height, components) =
                    candidates.next()?;
                if candidates.next().is_some() {
                    return None;
                }
                Some(PreviewImage {
                    range: segment.range.start + relative_start..segment.range.start + relative_end,
                    width,
                    height,
                    components,
                })
            }),
        "catia_preview_images",
    )
}

/// Decode the unique `LastSaveVersion` tuple from summary-information segments.
/// Repeated identical copies collapse to one value; conflicting copies reject
/// the version instead of selecting by position.
#[cfg(test)]
fn last_save_version(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<LastSaveVersion>, CodecError> {
    let segments = finjpl_segments(ctx, &BodyExtent::whole(data))?;
    last_save_version_in_segments(ctx, data, &segments)
}

fn last_save_version_in_segments(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    segments: &[FinjplSegment],
) -> Result<Option<LastSaveVersion>, CodecError> {
    let mut versions = Vec::new();
    for segment in segments
        .iter()
        .filter(|segment| segment.type_word == 0x0101_0003)
    {
        if let Some(version) = parse_last_save_version(ctx, &data[segment.range.clone()])? {
            ctx.push_vec(&mut versions, version, "catia_last_save_versions")?;
        }
    }
    versions.dedup();
    Ok((versions.len() == 1).then(|| versions.remove(0)))
}

/// Enumerate exact `CATStorageProperty` external-document references from
/// project-flags segments.
pub(crate) fn external_references(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<ExternalReference>, CodecError> {
    let segments = finjpl_segments(ctx, &BodyExtent::whole(data))?;
    external_references_in_segments(ctx, data, &segments)
}

fn external_references_in_segments(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    segments: &[FinjplSegment],
) -> Result<Vec<ExternalReference>, CodecError> {
    const STORAGE: &[u8] = b"\x34\x12CATStorageProperty";
    let mut references = Vec::new();
    for segment in segments
        .iter()
        .filter(|segment| segment.kind() == FinjplKind::ProjectFlags)
    {
        let bytes = &data[segment.range.clone()];
        for (relative, value) in bytes.windows(STORAGE.len()).enumerate() {
            if value == STORAGE {
                if let Some((target_offset, target)) = parse_external_reference(bytes, relative) {
                    let target =
                        ctx.copy_retained_text(target, "catia_external_reference_target")?;
                    let reference = ExternalReference {
                        offset: target_offset + segment.range.start,
                        target,
                    };
                    ctx.push_vec(&mut references, reference, "catia_external_references")?;
                }
            }
        }
    }
    Ok(references)
}

fn parse_external_reference(data: &[u8], start: usize) -> Option<(usize, &str)> {
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
    (data.get(at) == Some(&0x9f) && is_catia_document_name(target)).then_some(())?;
    Some((target_offset, target))
}

/// Tag byte plus one-byte length that precede a length-prefixed ASCII string.
const LENGTH_PREFIXED_ASCII_HEADER: NonZeroU32 = NonZeroU32::MIN.saturating_add(1);

fn length_prefixed_ascii<'a>(data: &'a [u8], at: &mut usize) -> Option<&'a str> {
    (data.get(*at) == Some(&0x34)).then_some(())?;
    let length = usize::from(*data.get(*at + 1)?);
    let start = (*at).checked_add(2)?;
    let end = start.checked_add(length)?;
    let value = data.get(start..end)?;
    *at = end;
    value
        .is_ascii()
        .then(|| std::str::from_utf8(value).ok())
        .flatten()
}

fn is_catia_document_name(value: &str) -> bool {
    [".catpart", ".catproduct", ".catshape", ".cgr"]
        .iter()
        .any(|extension| {
            value.len() >= extension.len()
                && value[value.len() - extension.len()..].eq_ignore_ascii_case(extension)
        })
}

fn parse_last_save_version(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<LastSaveVersion>, CodecError> {
    let parsed = (|| {
        Some((
            tagged_ascii(data, b"<Version>", b"/<Version>")?
                .parse()
                .ok()?,
            tagged_ascii(data, b"<Release>", b"/<Release>")?
                .parse()
                .ok()?,
            tagged_ascii(data, b"<ServicePack>", b"/<ServicePack>")?
                .parse()
                .ok()?,
            tagged_ascii(data, b"<HotFix>", b"/<HotFix>")?
                .parse()
                .ok()?,
            tagged_ascii(data, b"<BuildDate>", b"/<BuildDate>")?,
        ))
    })();
    let Some((version, release, service_pack, hot_fix, build_date)) = parsed else {
        return Ok(None);
    };
    let build_date = ctx.copy_retained_text(build_date, "catia_last_save_build_date")?;
    Ok(Some(LastSaveVersion {
        version,
        release,
        service_pack,
        hot_fix,
        build_date,
    }))
}

fn tagged_ascii<'a>(data: &'a [u8], open: &[u8], close: &[u8]) -> Option<&'a str> {
    let start = find(data, open)? + open.len();
    let relative_end = find(&data[start..], close)?;
    let value = data.get(start..start + relative_end)?;
    value
        .is_ascii()
        .then(|| std::str::from_utf8(value).ok())
        .flatten()
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
pub(crate) fn e5_record_stream(data: &[u8]) -> Option<Range<usize>> {
    let body = outer_body_range(data)?;
    let mut markers = memchr::memmem::find_iter(body.bytes(), FINJPL_MARKER)
        .map(|relative| body.range.start + relative);
    let mut current = markers.next();
    let candidates = std::iter::from_fn(|| {
        while let Some(pos) = current {
            current = markers.next();
            let segment_end = current.unwrap_or(body.range.end);
            if let Some(type_word) = View::u32_be_at(data, pos + FINJPL_MARKER.len()) {
                return Some((pos..segment_end, type_word));
            }
        }
        None
    });
    select_e5_record_stream(data, body.range(), candidates)
}

fn outer_body_range(data: &[u8]) -> Option<BodyExtent<'_>> {
    data.starts_with(OUTER_MAGIC).then_some(())?;
    let directory_offset =
        usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET)?).ok()?;
    let directory_length =
        usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH)?).ok()?;
    BodyExtent::from_directory_pair(data, directory_offset, directory_length)
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
        .then(|| BodyExtent::tail_from(data, outer_hdr::FILL_FF))
        .flatten()
    })?;
    let range = body.range();
    let end = body
        .bytes()
        .windows(FINJPL_MARKER.len())
        .position(|bytes| bytes == FINJPL_MARKER)
        .map_or(range.end, |relative| range.start + relative);
    Some(range.start..end)
}

fn e5_record_stream_in_segments(
    data: &[u8],
    body: Range<usize>,
    segments: &[FinjplSegment],
) -> Option<Range<usize>> {
    select_e5_record_stream(
        data,
        body,
        segments
            .iter()
            .map(|segment| (segment.range.clone(), segment.type_word)),
    )
}

fn select_e5_record_stream(
    data: &[u8],
    body: Range<usize>,
    candidates: impl Iterator<Item = (Range<usize>, u32)>,
) -> Option<Range<usize>> {
    let preamble = outer_preamble_range(data)?;
    if coherent_e5_record_count(&data[preamble.clone()]) >= 10 {
        return Some(preamble);
    }
    let mut best: Option<(usize, bool, Range<usize>)> = None;
    let mut tied = false;
    for (range, type_word) in candidates {
        if range.start < body.start || range.end > body.end {
            continue;
        }
        let count = coherent_e5_record_count(&data[range.clone()]);
        if count < 10 {
            continue;
        }
        let preferred = type_word == 0x0000_008e;
        match &best {
            None => {
                best = Some((count, preferred, range));
                tied = false;
            }
            Some((best_count, best_preferred, _))
                if count > *best_count
                    || (count == *best_count && preferred && !best_preferred) =>
            {
                best = Some((count, preferred, range));
                tied = false;
            }
            Some((best_count, best_preferred, _))
                if count == *best_count && preferred == *best_preferred =>
            {
                tied = true;
            }
            _ => {}
        }
    }
    if tied {
        None
    } else {
        best.map(|(_, _, range)| range)
    }
}

/// Count the longest declared-stride E5 walk in a bounded byte region.
///
/// A stream may place the unframed `05 08 01` coordinate roster between E5
/// records. No other gap is part of the walk.
fn coherent_e5_record_count(data: &[u8]) -> usize {
    let mut best = 0;
    let mut search = 0;
    while search < data.len() {
        let Some(relative) = data[search..]
            .windows(E5_MARKER.len())
            .position(|bytes| bytes == E5_MARKER)
        else {
            break;
        };
        let start = search + relative;
        let (count, consumed) = e5_record_walk_count(data, start);
        best = best.max(count);
        search = consumed.max(start + 1);
    }
    best
}

/// Return every valid `E5 0D 03` frame in a bounded stream region.
///
/// The complete E5 carrier stream may interleave these frames with other
/// framed E5 records. Route selection still uses [`coherent_e5_record_count`] because
/// it needs a contiguous declared-stride walk; carrier decoders need the
/// complete frame inventory instead.
pub(crate) fn all_e5_record_spans(data: &[u8]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut search = 0;
    std::iter::from_fn(move || loop {
        if search >= data.len() {
            return None;
        }
        let relative = data[search..]
            .windows(E5_MARKER.len())
            .position(|bytes| bytes == E5_MARKER)?;
        let start = search + relative;
        let Some(end) = e5_record_end(data, start) else {
            search = start + 1;
            continue;
        };
        search = end;
        return Some(start..end);
    })
}

fn e5_record_walk_count(data: &[u8], start: usize) -> (usize, usize) {
    let mut count = 0;
    let mut position = start;
    let mut consumed = start;
    while let Some(end) = e5_record_end(data, position) {
        count += 1;
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
    (count, consumed)
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

/// One physical extent of a logical stream. `phys_off` is measured from the
/// directory's physical storage base.
#[derive(Debug, Clone)]
struct Extent {
    /// Physical byte offset from the storage base. The base is zero for an
    /// outer directory and the nested magic offset for an inner directory.
    phys_off: u32,
    /// Physical byte length of this extent.
    phys_len: u32,
    /// Raw trailing extent flags word.
    flags: u32,
}

/// One catalogued logical stream.
#[derive(Debug, Clone)]
pub(crate) struct Descriptor {
    /// UTF-16LE ASCII name (`MainDataStream`, `SurfacicReps`, …).
    name: String,
    /// Offset of the descriptor header within the directory region.
    desc_offset: usize,
    /// Physical extents, in `log_off` order.
    extents: Vec<Extent>,
}

impl Descriptor {
    /// Logical stream length from the physical extents.
    fn logical_length(&self) -> u64 {
        self.extents
            .iter()
            .map(|extent| u64::from(extent.phys_len))
            .sum()
    }
}

/// A parsed stream directory. `inner` is the physical storage base: zero for
/// the outer directory and the nested `V5_CFV2` offset for an inner directory.
#[derive(Debug, Clone)]
pub(crate) struct InnerDir {
    /// File offset of the inner `V5_CFV2` magic.
    pub(crate) inner: usize,
    /// Catalogued streams.
    pub(crate) descriptors: Vec<Descriptor>,
}

/// Census counts used for variant identification and reporting.
#[derive(Debug, Clone, Default)]
pub(crate) struct Census {
    /// Contiguous stride-8 FBB runs in the BREP stream.
    pub(crate) fbb_runs: usize,
    /// Stride-8 FBB face rows in the BREP stream.
    pub(crate) fbb_face_rows: usize,
    /// `10 24 04 ff ff 00 00 00` standard edge-table delimiters in the BREP stream.
    pub(crate) edge_delimiters: usize,
    /// `05 08 01` vertex-record signatures in the BREP stream.
    pub(crate) vertex_markers: usize,
    /// Complete `a9 03` records in the outer preamble.
    pub(crate) a9_records: usize,
    /// `e5 0d 03` record-family markers in the outer body.
    e5_markers: usize,
}

/// Everything read from a `.CATPart`, shared by `inspect` and `decode`.
pub(crate) struct ContainerScan<'a> {
    /// The whole file image.
    pub(crate) data: Cow<'a, [u8]>,
    /// Outer directory offset (big-endian, from `+8`).
    pub(crate) outer_dir_offset: u32,
    /// Outer directory length (big-endian, from `+12`).
    outer_dir_length: u32,
    /// Parsed outer stream directory. Its descriptor physical offsets are
    /// absolute because `inner == 0`.
    outer: Option<InnerDir>,
    /// Parsed inner directory, when the file is nested and cataloguable.
    pub(crate) inner: Option<InnerDir>,
    /// Reconstructed BREP stream (largest `MainDataStream` + `SurfacicReps`).
    pub(crate) brep: Option<Vec<u8>>,
    /// Reconstructed canonical `MainDataStream`, which owns the standard FBB spine.
    pub(crate) main_data_stream: Option<Vec<u8>>,
    /// Exact JPEG previews extracted from summary-information framing.
    pub(crate) previews: Vec<PreviewImage>,
    /// Unique saved-by application version from summary information.
    pub(crate) last_save_version: Option<LastSaveVersion>,
    /// External CATIA documents named by storage properties.
    pub(crate) external_references: Vec<ExternalReference>,
    /// Every bounded outer FINJPL block in source order.
    pub(crate) finjpl_segments: Vec<FinjplSegment>,
    /// Exact model-container declarations from the outer `Data` stream.
    pub(crate) outer_container_declarations: Vec<OuterContainerDeclaration>,
    /// Record-family census.
    pub(crate) census: Census,
    /// Identified storage variant.
    pub(crate) variant: Variant,
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
pub(crate) fn consolidated_record_sources(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Vec<Vec<SourceExtent>>, CodecError> {
    let mut sources = Vec::new();
    let add_directory =
        |sources: &mut Vec<Vec<SourceExtent>>, directory: &InnerDir| -> Result<(), CodecError> {
            for descriptor in &directory.descriptors {
                let mut source = Vec::new();
                for extent in &descriptor.extents {
                    let Some(start) = directory.inner.checked_add(extent.phys_off as usize) else {
                        continue;
                    };
                    let Some(end) = start.checked_add(extent.phys_len as usize) else {
                        continue;
                    };
                    // A descriptor extent the image does not hold states no record
                    // source. The scanner reads every byte of an extent it accepts,
                    // so it never receives a shortened one.
                    if let Some(extent) = SourceExtent::within(&scan.data, start, end) {
                        ctx.push_vec(&mut source, extent, "catia_record_source_extents")?;
                    }
                }
                if !source.is_empty() && !sources.contains(&source) {
                    ctx.push_vec(sources, source, "catia_record_sources")?;
                }
            }
            Ok(())
        };

    if let Some(outer) = scan.outer.as_ref() {
        add_directory(&mut sources, outer)?;
    } else {
        let outer_end = scan
            .inner
            .as_ref()
            .map(|directory| directory.inner)
            .or_else(|| outer_stream_directory_range(&scan.data).map(|range| range.start))
            .unwrap_or(scan.data.len());
        let preamble = (outer_hdr::FILL_FF < outer_end)
            .then(|| SourceExtent::within(&scan.data, outer_hdr::FILL_FF, outer_end))
            .flatten();
        if let Some(preamble) = preamble {
            let mut source = Vec::new();
            ctx.push_vec(&mut source, preamble, "catia_record_source_extents")?;
            ctx.push_vec(&mut sources, source, "catia_record_sources")?;
        }
    }
    if let Some(inner) = scan.inner.as_ref() {
        add_directory(&mut sources, inner)?;
    }

    let whole = (sources.is_empty() && scan.data.len() > outer_hdr::FILL_FF)
        .then(|| SourceExtent::within(&scan.data, outer_hdr::FILL_FF, scan.data.len()))
        .flatten();
    if let Some(whole) = whole {
        let mut source = Vec::new();
        ctx.push_vec(&mut source, whole, "catia_record_source_extents")?;
        ctx.push_vec(&mut sources, source, "catia_record_sources")?;
    }
    Ok(sources)
}

/// Flatten the descriptor-scoped source inventory without changing logical
/// source or extent order.
#[cfg(test)]
pub(crate) fn consolidated_record_ranges(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Vec<Range<usize>>, CodecError> {
    ctx.collect_vec(
        consolidated_record_sources(ctx, scan)?
            .into_iter()
            .flatten()
            .map(|extent| extent.range()),
        "catia_record_ranges",
    )
}

/// Reconstruct each catalogued logical stream as an independent record source.
///
/// Records cannot establish adjacency or one object-id namespace across two
/// descriptors. A container without a parsed directory has one unnamed source:
/// its bounded outer preamble.
pub(crate) fn logical_record_streams(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut streams = Vec::new();
    for directory in [scan.outer.as_ref(), scan.inner.as_ref()]
        .into_iter()
        .flatten()
    {
        for descriptor in &directory.descriptors {
            let stream = reconstruct_logical_stream(ctx, &scan.data, descriptor, directory.inner)?;
            if !stream.is_empty() {
                ctx.push_vec(&mut streams, stream, "catia_logical_record_streams")?;
            }
        }
    }
    if streams.is_empty() {
        if let Some(range) = outer_preamble_range(&scan.data) {
            let stream =
                ctx.copy_retained_slice(&scan.data[range], "catia_outer_preamble_stream")?;
            ctx.push_vec(&mut streams, stream, "catia_logical_record_streams")?;
        }
    }
    Ok(streams)
}

/// Whether a byte prefix is a `.CATPart`: the `V5_CFV2\0` outer magic is unique
/// to Dassault's container and is a conclusive signal on its own.
pub(crate) fn looks_like_catia(prefix: &[u8]) -> bool {
    prefix.starts_with(OUTER_MAGIC)
}

/// Return maximal contiguous stride-8 FBB groups in source order.
pub(crate) fn fbb_run_ranges(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Vec<Range<usize>>, CodecError> {
    let mut ranges = Vec::new();
    let mut position = 0;
    while position + fbb_row::LEN <= body.len() {
        if is_fbb_row(&body[position..]) {
            let start = position;
            while position + fbb_row::LEN <= body.len() && is_fbb_row(&body[position..]) {
                position += fbb_row::LEN;
            }
            ctx.push_vec(&mut ranges, start..position, "catia_fbb_run_ranges")?;
        } else {
            position += 1;
        }
    }
    Ok(ranges)
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
pub(crate) fn parse_stream_directory(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<InnerDir>, CodecError> {
    if data.len() < inner_hdr::LEN {
        return Ok(None);
    }
    let Some((inner, dir_offset, b)) = (|| {
        let inner = find_from(data, OUTER_MAGIC, OUTER_MAGIC.len())?;
        let a = usize::try_from(View::u32_be_at(
            data,
            inner.checked_add(inner_hdr::DIRECTORY_OFFSET_DELTA)?,
        )?)
        .ok()?;
        let b = usize::try_from(View::u32_be_at(
            data,
            inner.checked_add(inner_hdr::DIRECTORY_LENGTH)?,
        )?)
        .ok()?;
        Some((inner, inner.checked_add(a)?, b))
    })() else {
        return Ok(None);
    };
    let Some(magic_end) = dir_offset.checked_add(DIR_MAGIC.len()) else {
        return Ok(None);
    };
    if data.get(dir_offset..magic_end) != Some(DIR_MAGIC) {
        return Ok(None);
    }
    if b == 0 || dir_offset.checked_add(b).is_none_or(|end| end > data.len()) {
        return Ok(None);
    }
    parse_directory_region(ctx, data, inner, dir_offset, b)
}

/// Parse the outer `CATIA_V5 CB0001` stream directory. Physical extent offsets
/// in its descriptors are absolute file offsets.
pub(crate) fn parse_outer_stream_directory(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<InnerDir>, CodecError> {
    Ok(parse_outer_stream_directory_with_range(ctx, data)?.map(|(_, directory)| directory))
}

/// Parse and return the exact outer stream-directory byte range.
pub(crate) fn outer_stream_directory_range(data: &[u8]) -> Option<Range<usize>> {
    let dir_offset = usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET)?).ok()?;
    let dir_length = usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH)?).ok()?;
    let dir_end = dir_offset.checked_add(dir_length)?;
    (dir_end == data.len() && directory_region_has_descriptor(data, 0, dir_offset, dir_length))
        .then_some(dir_offset..dir_end)
}

fn parse_outer_stream_directory_with_range(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<(Range<usize>, InnerDir)>, CodecError> {
    let Some((dir_offset, dir_length, dir_end)) = (|| {
        let dir_offset =
            usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET)?).ok()?;
        let dir_length =
            usize::try_from(View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH)?).ok()?;
        let dir_end = dir_offset.checked_add(dir_length)?;
        (dir_end == data.len()).then_some((dir_offset, dir_length, dir_end))
    })() else {
        return Ok(None);
    };
    let Some(directory) = parse_directory_region(ctx, data, 0, dir_offset, dir_length)? else {
        return Ok(None);
    };
    Ok(Some((dir_offset..dir_end, directory)))
}

fn parse_directory_region(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    physical_base: usize,
    dir_offset: usize,
    dir_length: usize,
) -> Result<Option<InnerDir>, CodecError> {
    let Some(dir_end) = dir_offset.checked_add(dir_length) else {
        return Ok(None);
    };
    let Some(magic_end) = dir_offset.checked_add(16) else {
        return Ok(None);
    };
    if dir_length == 0 || dir_end > data.len() || data.get(dir_offset..magic_end) != Some(DIR_MAGIC)
    {
        return Ok(None);
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
            if let Some((extents, cum)) = parse_extents(ctx, dirbuf, o, k, physical_base, file_len)?
            {
                if cum > 0 && o >= stream_desc::EXTENT_COUNT {
                    let ds = o - stream_desc::EXTENT_COUNT;
                    let logical_length =
                        View::u32_be_at(dirbuf, ds + stream_desc::LOGICAL_STREAM_LENGTH)
                            .unwrap_or(0);
                    if logical_length as usize == cum {
                        let name = descriptor_name(ctx, dirbuf, ds)?;
                        ctx.push_vec(
                            &mut descriptors,
                            Descriptor {
                                name,
                                desc_offset: ds,
                                extents,
                            },
                            "catia_directory_descriptors",
                        )?;
                    }
                }
            }
        }
        o += 1;
    }

    if descriptors.is_empty() {
        return Ok(None);
    }
    Ok(Some(InnerDir {
        inner: physical_base,
        descriptors,
    }))
}

fn directory_region_has_descriptor(
    data: &[u8],
    physical_base: usize,
    dir_offset: usize,
    dir_length: usize,
) -> bool {
    let Some(dir_end) = dir_offset.checked_add(dir_length) else {
        return false;
    };
    let Some(magic_end) = dir_offset.checked_add(DIR_MAGIC.len()) else {
        return false;
    };
    if dir_length == 0 || dir_end > data.len() || data.get(dir_offset..magic_end) != Some(DIR_MAGIC)
    {
        return false;
    }
    let dirbuf = &data[dir_offset..dir_end];
    if dirbuf.len() < 4 {
        return false;
    }
    for o in 0..=dirbuf.len() - 4 {
        let Some(k) = View::u32_be_at(dirbuf, o).and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        let Some(extents_end) = k
            .checked_mul(extent::LEN)
            .and_then(|bytes| o.checked_add(4)?.checked_add(bytes))
        else {
            continue;
        };
        if k == 0 || extents_end > dirbuf.len() || o < stream_desc::EXTENT_COUNT {
            continue;
        }
        let Some(cum) = validate_extents(dirbuf, o, k, physical_base, data.len()) else {
            continue;
        };
        let ds = o - stream_desc::EXTENT_COUNT;
        if cum > 0
            && View::u32_be_at(dirbuf, ds + stream_desc::LOGICAL_STREAM_LENGTH)
                .is_some_and(|length| length as usize == cum)
        {
            return true;
        }
    }
    false
}

/// Validate the `k` 20-byte extent structs beginning at `o + 4`; returns the
/// extents and their cumulative logical length, or `None` if any extent fails a
/// gate (`log_off` cumulative from 0, `log_len == phys_len`, physically in range).
fn parse_extents(
    ctx: &DecodeContext<'_>,
    dirbuf: &[u8],
    o: usize,
    k: usize,
    physical_base: usize,
    file_len: usize,
) -> Result<Option<(Vec<Extent>, usize)>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(k),
        "catia_extent_validation",
    )?;
    let Some(cum) = validate_extents(dirbuf, o, k, physical_base, file_len) else {
        return Ok(None);
    };
    let mut extents = Vec::new();
    ctx.reserve_vec(&mut extents, k, "catia_directory_extents")?;
    for i in 0..k {
        let Some((extent, _, _)) = read_extent_fields(dirbuf, o, i) else {
            return Ok(None);
        };
        extents.push(extent);
    }
    Ok(Some((extents, cum)))
}

fn validate_extents(
    dirbuf: &[u8],
    o: usize,
    k: usize,
    physical_base: usize,
    file_len: usize,
) -> Option<usize> {
    let mut cum: usize = 0;
    for i in 0..k {
        let (extent, log_len, log_off) = read_extent_fields(dirbuf, o, i)?;
        let phys_end = physical_base
            .checked_add(extent.phys_off as usize)
            .and_then(|start| start.checked_add(extent.phys_len as usize));
        if extent.phys_len == 0
            || phys_end.is_none_or(|end| end > file_len)
            || log_off as usize != cum
            || log_len != extent.phys_len
        {
            return None;
        }
        let next = cum.checked_add(log_len as usize)?;
        cum = next;
    }
    Some(cum)
}

fn read_extent_fields(dirbuf: &[u8], o: usize, index: usize) -> Option<(Extent, u32, u32)> {
    let base = o
        .checked_add(4)?
        .checked_add(extent::LEN.checked_mul(index)?)?;
    Some((
        Extent {
            phys_off: View::u32_be_at(dirbuf, base + extent::PHYS_OFF)?,
            phys_len: View::u32_be_at(dirbuf, base + extent::PHYS_LEN)?,
            flags: View::u32_be_at(dirbuf, base + extent::FLAGS)?,
        },
        View::u32_be_at(dirbuf, base + extent::LOG_LEN)?,
        View::u32_be_at(dirbuf, base + extent::LOG_OFF)?,
    ))
}

/// Read a descriptor's UTF-16LE ASCII stream name from one of its two framed
/// name locations.
///
/// The descriptor tail is a two-byte UTF-16LE terminator followed by one zero
/// padding byte. The name is the complete run of printable ASCII code units
/// immediately before that tail. This end anchor keeps unrelated UTF-16 text
/// elsewhere in the descriptor from becoming the stream name.
fn descriptor_name(
    ctx: &DecodeContext<'_>,
    dirbuf: &[u8],
    ds: usize,
) -> Result<String, CodecError> {
    struct Utf16Ascii<'a>(&'a [u8]);
    impl std::fmt::Display for Utf16Ascii<'_> {
        fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for pair in self.0.chunks_exact(2) {
                write!(out, "{}", char::from(pair[0]))?;
            }
            Ok(())
        }
    }
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
                return ctx.format_retained(
                    format_args!("{}", Utf16Ascii(name_bytes)),
                    "catia_descriptor_name",
                );
            }
        }
    }

    // Older directory headers place an unframed name at ds+0x10. Admit this
    // form only when the name closes with a UTF-16LE terminator and the rest
    // of the header before the extent count is zero.
    let Some(header_name_start) = ds.checked_add(0x10) else {
        return Ok(String::new());
    };
    let Some(header_end) = ds.checked_add(stream_desc::EXTENT_COUNT) else {
        return Ok(String::new());
    };
    let Some(header_name) = dirbuf.get(header_name_start..header_end) else {
        return Ok(String::new());
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
        return Ok(String::new());
    }

    ctx.format_retained(
        format_args!("{}", Utf16Ascii(&header_name[..name_len])),
        "catia_descriptor_name",
    )
}

/// Concatenate a logical stream's physical extents in `log_off` order.
fn reconstruct_logical_stream(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    descriptor: &Descriptor,
    inner: usize,
) -> Result<Vec<u8>, CodecError> {
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
        return Ok(Vec::new());
    };
    let bytes = cadmpeg_core::decode::u64_from_index(logical_length);
    ctx.charge_retained(bytes, "catia_logical_stream_bytes")?;
    let mut out = Vec::new();
    ctx.reserve_vec(&mut out, logical_length, "catia_logical_stream_bytes")?;
    for extent in &descriptor.extents {
        let start = inner + extent.phys_off as usize;
        let end = start + extent.phys_len as usize;
        out.extend_from_slice(&data[start..end]);
    }
    Ok(out)
}

/// Decode model-container declarations whose UUIDs select named outer streams.
pub(crate) fn outer_container_declarations(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    outer: &InnerDir,
) -> Result<Vec<OuterContainerDeclaration>, CodecError> {
    let mut data_descriptors = outer
        .descriptors
        .iter()
        .filter(|descriptor| descriptor.name == "Data");
    let Some(data_descriptor) = data_descriptors.next() else {
        return Ok(Vec::new());
    };
    if data_descriptors.next().is_some() {
        return Ok(Vec::new());
    }
    let logical = reconstruct_logical_stream(ctx, data, data_descriptor, outer.inner)?;
    parse_outer_container_declarations(ctx, &logical, &outer.descriptors)
}

/// Select the unique declared outer container whose physical extent contains
/// the complete file range.
#[must_use]
pub(crate) fn outer_container_for_extent<'a>(
    outer: &InnerDir,
    declarations: &'a [OuterContainerDeclaration],
    byte_offset: u64,
    byte_len: u64,
) -> Option<&'a OuterContainerDeclaration> {
    let byte_end = byte_offset.checked_add(byte_len)?;
    let physical_base = cadmpeg_core::decode::u64_from_index(outer.inner);
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
    ctx: &DecodeContext<'_>,
    data: &[u8],
    descriptors: &[Descriptor],
) -> Result<Vec<OuterContainerDeclaration>, CodecError> {
    const HEADER: &[u8] = b"\x01\x00\x03\x00";
    const PREFIX: &[u8] = b"\x01\x00\x6c\x00\x02\x00\x00\x00";
    const CLASS_BLOCK: &[u8] = b"\x02\x00\x81\x20";
    const TERMINAL: &[u8] = b"\x03\x00\xf7\x00\x03\x00\x00\x00";

    let mut declarations = Vec::new();
    if data.len() < 64 {
        return Ok(declarations);
    }
    for start in 0..data.len() - 64 {
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
        let canonical_stream_name = ctx.format_retained(
            format_args!("{first:x}_{middle:08x}_{last:x}"),
            "catia_container_stream_name",
        )?;
        let prefixed_stream_name = ctx.format_retained(
            format_args!("_{canonical_stream_name}"),
            "catia_container_stream_name",
        )?;
        let stream_name = match (
            descriptors
                .iter()
                .any(|descriptor| descriptor.name == canonical_stream_name),
            descriptors
                .iter()
                .any(|descriptor| descriptor.name == prefixed_stream_name),
        ) {
            (true, false) => canonical_stream_name,
            (false, true) => prefixed_stream_name,
            (false, false) | (true, true) => continue,
        };
        let Some(ordinal) = View::u32_le_at(data, start + 12) else {
            continue;
        };
        let class_name = ctx.copy_retained_text(class_name, "catia_container_class_name")?;
        let base_class = ctx.copy_retained_text(base_class, "catia_container_base_class")?;
        ctx.push_vec(
            &mut declarations,
            OuterContainerDeclaration {
                data_offset: start,
                ordinal,
                class_name,
                base_class,
                stream_name,
            },
            "catia_container_declarations",
        )?;
    }
    let selected_streams = ctx.collect_hash_set(
        declarations
            .iter()
            .map(|declaration| declaration.stream_name.as_str()),
        "catia_container_selected_streams",
    )?;
    if selected_streams.len() != declarations.len() {
        return Ok(Vec::new());
    }
    Ok(declarations)
}

fn declaration_class_pair(data: &[u8]) -> Option<(&str, &str)> {
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
        std::str::from_utf8(first).ok()?,
        std::str::from_utf8(second).ok()?,
    ))
}

/// Reconstruct the logical BREP buffer: the uniquely largest canonical
/// `MainDataStream` followed by the uniquely largest canonical `SurfacicReps`
/// ([spec §3.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#34-nested-container-stream-directory)). Both are required. A directory that
/// catalogues the BREP body carries both canonical streams; the contiguous-body
/// exception has neither and returns `None`.
fn brep_stream(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    dir: &InnerDir,
) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(mut out) = main_data_stream(ctx, data, dir)? else {
        return Ok(None);
    };
    let Some(surf) = unique_largest_descriptor(
        dir.descriptors
            .iter()
            .filter(|descriptor| descriptor.name == "SurfacicReps"),
    ) else {
        return Ok(None);
    };
    let surface = reconstruct_logical_stream(ctx, data, surf, dir.inner)?;
    ctx.extend_retained_bytes(&mut out, &surface, "catia_brep_surface_bytes")?;
    Ok(Some(out))
}

/// Reconstruct the unique canonical `MainDataStream`, which owns the FBB
/// topology spine. The surface stream is deliberately excluded: its numeric
/// payload may contain byte sequences that resemble FBB rows but cannot assign
/// topology faces.
fn main_data_stream(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    dir: &InnerDir,
) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(main) = unique_largest_descriptor(
        dir.descriptors
            .iter()
            .filter(|descriptor| descriptor.name == "MainDataStream"),
    ) else {
        return Ok(None);
    };
    Ok(Some(reconstruct_logical_stream(
        ctx, data, main, dir.inner,
    )?))
}

fn unique_largest_descriptor<'a>(
    descriptors: impl IntoIterator<Item = &'a Descriptor>,
) -> Option<&'a Descriptor> {
    let mut selected = None;
    let mut selected_length = 0;
    let mut equal_count = 0;
    for descriptor in descriptors {
        let logical_length = descriptor.logical_length();
        if selected.is_none() || logical_length > selected_length {
            selected = Some(descriptor);
            selected_length = logical_length;
            equal_count = 1;
        } else if logical_length == selected_length {
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
    ctx: &DecodeContext<'_>,
    inner: Option<&InnerDir>,
    brep: Option<&[u8]>,
    main_data_stream: Option<&[u8]>,
    census: &Census,
    coherent_e5: bool,
) -> Result<Variant, CodecError> {
    Ok(match (inner, brep) {
        // A standard edge table establishes a complete nested FBB body. It
        // takes precedence over an unrelated E5 stream in the same container.
        (Some(_), Some(brep)) if census.fbb_runs > 0 => {
            let variant = identify_fbb_variant(ctx, main_data_stream.unwrap_or(brep), census)?;
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
    })
}

fn identify_fbb_variant(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
    census: &Census,
) -> Result<Variant, CodecError> {
    if crate::families::standard::fbb::standard_edge_count(ctx, brep)?.is_some() {
        return Ok(Variant::StandardNested);
    }
    if crate::families::standard::fbb::fbb_only_edge_count(ctx, brep)?.is_some() {
        return Ok(Variant::FbbOnly);
    }
    if census.edge_delimiters == 0 && census.vertex_markers > 0 {
        Ok(Variant::FbbOnly)
    } else {
        Ok(Variant::Unknown)
    }
}

/// Identify a whole `.CATPart` byte image.
pub(crate) fn scan_bytes<'a>(
    ctx: &DecodeContext<'_>,
    data: impl Into<Cow<'a, [u8]>>,
) -> Result<ContainerScan<'a>, CodecError> {
    let data = data.into();
    let outer_dir_offset = View::u32_be_at(&data, outer_hdr::DIRECTORY_OFFSET).unwrap_or(0);
    let outer_dir_length = View::u32_be_at(&data, outer_hdr::DIRECTORY_LENGTH).unwrap_or(0);

    let outer = parse_outer_stream_directory(ctx, &data)?;
    let inner = parse_stream_directory(ctx, &data)?;
    let brep = match inner.as_ref() {
        Some(dir) => brep_stream(ctx, &data, dir)?,
        None => None,
    };
    let main_data_stream = match inner.as_ref() {
        Some(dir) => main_data_stream(ctx, &data, dir)?,
        None => None,
    };
    let outer_body = outer_body_range(&data);
    let finjpl_segments = match outer_body.as_ref() {
        Some(body) => finjpl_segments(ctx, body)?,
        None => Vec::new(),
    };
    let previews = preview_images_in_segments(ctx, &data, &finjpl_segments)?;
    let last_save_version = last_save_version_in_segments(ctx, &data, &finjpl_segments)?;
    let external_references = external_references_in_segments(ctx, &data, &finjpl_segments)?;
    let outer_container_declarations = match outer.as_ref() {
        Some(directory) => outer_container_declarations(ctx, &data, directory)?,
        None => Vec::new(),
    };

    let a9_records = match outer_preamble_range(&data) {
        Some(range) => {
            crate::families::zero_entity::records::zero_entity_record_inventory_in_range(
                ctx, &data, range,
            )?
            .len()
        }
        None => 0,
    };
    let mut census = Census {
        a9_records,
        e5_markers: outer_body
            .as_ref()
            .map_or(0, |body| count_subslice(body.bytes(), E5_MARKER)),
        ..Default::default()
    };
    if let Some(b) = main_data_stream.as_deref() {
        let fbb_ranges = fbb_run_ranges(ctx, b)?;
        census.fbb_runs = fbb_ranges.len();
        census.fbb_face_rows = fbb_ranges
            .iter()
            .map(|range| (range.end - range.start) / fbb_row::LEN)
            .sum();
        census.edge_delimiters = count_subslice(b, EDGE_DELIMITER);
        census.vertex_markers = count_subslice(b, VERTEX_MARKER);
    }

    let variant = identify_variant(
        ctx,
        inner.as_ref(),
        brep.as_deref(),
        main_data_stream.as_deref(),
        &census,
        outer_body.is_some_and(|body| {
            e5_record_stream_in_segments(&data, body.range(), &finjpl_segments).is_some()
        }),
    )?;

    Ok(ContainerScan {
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
        census,
        variant,
    })
}

/// Build a [`ContainerSummary`] enumerating the outer and inner directories'
/// streams and the identified variant.
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<ContainerSummary, CodecError> {
    struct ExtentFlags<'a>(&'a [Extent]);
    impl std::fmt::Display for ExtentFlags<'_> {
        fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for (index, extent) in self.0.iter().enumerate() {
                if index != 0 {
                    out.write_str(",")?;
                }
                write!(out, "0x{:08x}", extent.flags)?;
            }
            Ok(())
        }
    }
    let mut entries = Vec::new();

    for (directory, dir) in [
        ("outer", scan.outer.as_ref()),
        ("inner", scan.inner.as_ref()),
    ] {
        let Some(dir) = dir else { continue };
        for d in &dir.descriptors {
            let mut attributes = BTreeMap::new();
            crate::resource::string_attribute(
                ctx,
                &mut attributes,
                "directory",
                format_args!("{directory}"),
                "catia_summary_attribute",
            )?;
            crate::resource::string_attribute(
                ctx,
                &mut attributes,
                "desc_offset",
                format_args!("{}", d.desc_offset),
                "catia_summary_attribute",
            )?;
            crate::resource::string_attribute(
                ctx,
                &mut attributes,
                "extent_count",
                format_args!("{}", d.extents.len()),
                "catia_summary_attribute",
            )?;
            crate::resource::string_attribute(
                ctx,
                &mut attributes,
                "extent_flags",
                format_args!("{}", ExtentFlags(&d.extents)),
                "catia_summary_attribute",
            )?;
            if directory == "outer" {
                if let Some(declaration) = scan
                    .outer_container_declarations
                    .iter()
                    .find(|declaration| declaration.stream_name == d.name)
                {
                    crate::resource::string_attribute(
                        ctx,
                        &mut attributes,
                        "container_class",
                        format_args!("{}", declaration.class_name),
                        "catia_summary_attribute",
                    )?;
                    crate::resource::string_attribute(
                        ctx,
                        &mut attributes,
                        "container_base_class",
                        format_args!("{}", declaration.base_class),
                        "catia_summary_attribute",
                    )?;
                    crate::resource::string_attribute(
                        ctx,
                        &mut attributes,
                        "container_ordinal",
                        format_args!("{}", declaration.ordinal),
                        "catia_summary_attribute",
                    )?;
                    crate::resource::string_attribute(
                        ctx,
                        &mut attributes,
                        "container_data_offset",
                        format_args!("{}", declaration.data_offset),
                        "catia_summary_attribute",
                    )?;
                }
            }
            let phys = d.logical_length();
            let name = if d.name.is_empty() {
                ctx.format_retained(
                    format_args!("{directory}-stream@{}", d.desc_offset),
                    "catia_summary_entry_name",
                )?
            } else {
                ctx.copy_retained_text(&d.name, "catia_summary_entry_name")?
            };
            ctx.push_vec(
                &mut entries,
                ContainerEntry {
                    name,
                    role: ContainerRole::Stream,
                    storage: EntryStorage::verbatim(VerbatimLabel::None, phys),
                    attributes,
                },
                "catia_summary_entries",
            )?;
        }
    }
    for (index, preview) in scan.previews.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "file_offset",
            format_args!("{}", preview.range.start),
            "catia_summary_attribute",
        )?;
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "width",
            format_args!("{}", preview.width),
            "catia_summary_attribute",
        )?;
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "height",
            format_args!("{}", preview.height),
            "catia_summary_attribute",
        )?;
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "components",
            format_args!("{}", preview.components),
            "catia_summary_attribute",
        )?;
        let name = ctx.format_retained(
            format_args!("CATPreview#{index}"),
            "catia_summary_entry_name",
        )?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name,
                role: ContainerRole::Preview,
                storage: EntryStorage::Compressed {
                    method: CompressionMethod::Jpeg,
                    stored: Some((preview.range.end - preview.range.start) as u64),
                    expanded: None,
                },
                attributes,
            },
            "catia_summary_entries",
        )?;
    }
    for reference in &scan.external_references {
        let mut attributes = BTreeMap::new();
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "file_offset",
            format_args!("{}", reference.offset),
            "catia_summary_attribute",
        )?;
        let storage = EntryStorage::framed_by(
            VerbatimLabel::None,
            reference.target.as_str().into(),
            LENGTH_PREFIXED_ASCII_HEADER,
        );
        let name = ctx.copy_retained_text(&reference.target, "catia_summary_entry_name")?;
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name,
                role: ContainerRole::ExternalReference,
                storage,
                attributes,
            },
            "catia_summary_entries",
        )?;
    }
    for (index, segment) in scan.finjpl_segments.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "file_offset",
            format_args!("{}", segment.range.start),
            "catia_summary_attribute",
        )?;
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "type_word",
            format_args!("0x{:08x}", segment.type_word),
            "catia_summary_attribute",
        )?;
        let family = match segment.kind() {
            FinjplKind::Storage => "storage",
            FinjplKind::ProjectFlags => "project-flags",
            FinjplKind::Other => "other",
        };
        crate::resource::string_attribute(
            ctx,
            &mut attributes,
            "family",
            format_args!("{family}"),
            "catia_summary_attribute",
        )?;
        let name = match &segment.name {
            Some(name) => ctx.copy_retained_text(name, "catia_summary_entry_name")?,
            None => {
                ctx.format_retained(format_args!("FINJPL#{index}"), "catia_summary_entry_name")?
            }
        };
        ctx.push_vec(
            &mut entries,
            ContainerEntry {
                name,
                role: ContainerRole::FinjplSegment,
                storage: EntryStorage::verbatim(
                    VerbatimLabel::None,
                    (segment.range.end - segment.range.start) as u64,
                ),
                attributes,
            },
            "catia_summary_entries",
        )?;
    }

    let notes = notes(ctx, scan)?;

    let matched = crate::dialect::classify(ctx, scan)?;
    let mut losses = Vec::new();
    if let Some(loss) = crate::dialect::dialect_loss(ctx, &matched)? {
        ctx.push_vec(&mut losses, loss, "catia_summary_losses")?;
    }
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        cadmpeg_ir::ContainerKind::V5Cfv2,
        entries,
        losses,
        notes,
    ))
}

/// Build the diagnostic notes shared by inspection and decode reports.
pub(crate) fn notes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<String>, CodecError> {
    let mut notes = Vec::new();
    let outer = ctx.format_retained(format_args!(
            "outer V5_CFV2 container: directory offset {} + length {} = {} (file size {}); variant: {}",
            scan.outer_dir_offset,
            scan.outer_dir_length,
            u64::from(scan.outer_dir_offset) + u64::from(scan.outer_dir_length),
            scan.data.len(),
            scan.variant.description(),
        ), "catia_container_note")?;
    ctx.push_vec(&mut notes, outer, "catia_container_notes")?;

    if let Some(dir) = &scan.outer {
        let note = ctx.format_retained(
            format_args!(
                "outer CATIA_V5 CB0001 directory with {} stream(s)",
                dir.descriptors.len()
            ),
            "catia_container_note",
        )?;
        ctx.push_vec(&mut notes, note, "catia_container_notes")?;
    }

    match &scan.inner {
        Some(dir) => {
            let note = ctx.format_retained(format_args!(
                    "nested V5_CFV2 at file offset {} with a CATIA_V5 CB0001 directory of {} stream(s)",
                    dir.inner, dir.descriptors.len(),
                ), "catia_container_note")?;
            ctx.push_vec(&mut notes, note, "catia_container_notes")?;
        }
        None => {
            let note = ctx.copy_retained_text(
                "no nested V5_CFV2 sub-container (outer-preamble record families only)",
                "catia_container_note",
            )?;
            ctx.push_vec(&mut notes, note, "catia_container_notes")?;
        }
    }

    if scan.brep.is_some() {
        let note =
            ctx.format_retained(
                format_args!(
                "reconstructed BREP stream from MainDataStream + SurfacicReps: {} FBB group(s) \
                 containing {} face row(s), {} vertex record(s), {} edge-table delimiter(s)",
                scan.census.fbb_runs, scan.census.fbb_face_rows,
                scan.census.vertex_markers, scan.census.edge_delimiters,
            ),
                "catia_container_note",
            )?;
        ctx.push_vec(&mut notes, note, "catia_container_notes")?;
    }
    if scan.census.a9_records > 0 || scan.census.e5_markers > 0 {
        let note = ctx.format_retained(
            format_args!(
                "record-family census: {} a9 03, {} e5 0d 03",
                scan.census.a9_records, scan.census.e5_markers
            ),
            "catia_container_note",
        )?;
        ctx.push_vec(&mut notes, note, "catia_container_notes")?;
    }
    if let Some(version) = &scan.last_save_version {
        let note = ctx.format_retained(
            format_args!(
                "last saved by CATIA V{}R{} SP{} HF{} ({})",
                version.version,
                version.release,
                version.service_pack,
                version.hot_fix,
                version.build_date
            ),
            "catia_container_note",
        )?;
        ctx.push_vec(&mut notes, note, "catia_container_notes")?;
    }
    let note = ctx.copy_retained_text(
        "container-level enumeration; `decode` applies the identified storage family's \
         standard, freeform, E5, zero-entity, or metadata-fallback route",
        "catia_container_note",
    )?;
    ctx.push_vec(&mut notes, note, "catia_container_notes")?;
    Ok(notes)
}

#[cfg(test)]
mod tests;
