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

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroU32;
use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::families::e5::records::{E5Frame, E5_MARKER};
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
    // Each marker closes the segment the previous marker opened.
    let mut segments = Vec::new();
    let mut open = None;
    for relative in
        ctx.find_bytes_iter(&data[body_start..end], FINJPL_MARKER, "catia_finjpl_scan")?
    {
        let pos = body_start + relative;
        if let Some(start) = open.replace(pos) {
            push_finjpl_segment(ctx, data, start..pos, &mut segments)?;
        }
    }
    if let Some(start) = open {
        push_finjpl_segment(ctx, data, start..end, &mut segments)?;
    }
    Ok(segments)
}

fn push_finjpl_segment(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    segments: &mut Vec<FinjplSegment>,
) -> Result<(), CodecError> {
    let Some(type_word) = View::u32_be_at(data, range.start + FINJPL_MARKER.len()) else {
        return Ok(());
    };
    let name = finjpl_primary_name(ctx, data, range.start, range.end)?;
    ctx.push_vec(
        segments,
        FinjplSegment {
            range,
            type_word,
            name,
        },
        "catia_finjpl_segments",
    )
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
    if value.is_empty()
        || !ctx.all_by(
            value,
            |byte| Ok(matches!(byte, 0x20..=0x7e)),
            "catia_finjpl_name_scan",
        )?
    {
        return Ok(None);
    }
    let Ok(value) = ctx.validate_utf8(value, "catia_finjpl_name_scan")? else {
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
    let mut previews = Vec::new();
    for segment in ctx
        .admit_iter(segments, "catia_preview_segment_scan")?
        .filter(|segment| segment.type_word == 0x0101_0003)
    {
        let bytes = &data[segment.range.clone()];
        let mut selected = None;
        let mut ambiguous = false;
        // `ff d8` cannot overlap itself, so its matches are every SOI start;
        // a candidate also needs the following marker byte.
        for start in ctx.find_bytes_iter(bytes, &[0xff, 0xd8], "catia_jpeg_candidate_scan")? {
            if bytes.get(start + 2) != Some(&0xff) {
                continue;
            }
            if let Some((end, width, height, components)) = jpeg_extent(ctx, bytes, start)? {
                if selected.is_some() {
                    ambiguous = true;
                    break;
                }
                selected = Some(PreviewImage {
                    range: segment.range.start + start..segment.range.start + end,
                    width,
                    height,
                    components,
                });
            }
        }
        if !ambiguous {
            if let Some(preview) = selected {
                ctx.push_vec(&mut previews, preview, "catia_preview_images")?;
            }
        }
    }
    Ok(previews)
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
    let mut selected: Option<(LastSaveVersion, ScopedReservation<'_>)> = None;
    let mut segments = segments.iter();
    while let Some(segment) = ctx.next_charged(&mut segments, "catia_last_save_segment_scan")? {
        if segment.type_word != 0x0101_0003 {
            continue;
        }
        let mut version_storage = ctx.reserve_scoped(0, "catia_last_save_version_storage")?;
        let Some(version) = version_storage
            .with_storage(|| parse_last_save_version(ctx, &data[segment.range.clone()]))?
        else {
            continue;
        };
        match &selected {
            None => selected = Some((version, version_storage)),
            Some((existing, _)) => {
                let same = (
                    existing.version,
                    existing.release,
                    existing.service_pack,
                    existing.hot_fix,
                ) == (
                    version.version,
                    version.release,
                    version.service_pack,
                    version.hot_fix,
                ) && ctx.equal_bytes(
                    existing.build_date.as_bytes(),
                    version.build_date.as_bytes(),
                    "catia_last_save_version_compare",
                )?;
                if !same {
                    return Ok(None);
                }
            }
        }
    }
    match selected {
        Some((version, storage)) => {
            storage.commit()?;
            Ok(Some(version))
        }
        None => Ok(None),
    }
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
    for segment in ctx
        .admit_iter(segments, "catia_external_reference_segment_scan")?
        .filter(|segment| segment.kind() == FinjplKind::ProjectFlags)
    {
        let bytes = &data[segment.range.clone()];
        // The pattern has no border, so its matches are every occurrence.
        for relative in ctx.find_bytes_iter(bytes, STORAGE, "catia_external_reference_scan")? {
            if let Some((target_offset, target)) = parse_external_reference(ctx, bytes, relative)? {
                let target = ctx.copy_retained_text(target, "catia_external_reference_target")?;
                let reference = ExternalReference {
                    offset: target_offset + segment.range.start,
                    target,
                };
                ctx.push_vec(&mut references, reference, "catia_external_references")?;
            }
        }
    }
    Ok(references)
}

fn parse_external_reference<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    start: usize,
) -> Result<Option<(usize, &'a str)>, CodecError> {
    let mut at = start;
    let fixed = |at: &mut usize, expected: &[u8]| {
        let matched = data.get(*at..*at + expected.len()) == Some(expected);
        *at += expected.len();
        matched
    };
    if length_prefixed_ascii(ctx, data, &mut at)? != Some("CATStorageProperty")
        || !fixed(&mut at, &[0x80, 0x01, 0, 0, 0, 0])
        || !fixed(&mut at, &[0x22, 0x0c, 0, 0, 0, 0x34, 0x01, 0x01, 0x00])
        || length_prefixed_ascii(ctx, data, &mut at)? != Some("CATUnicodeString")
        || !fixed(&mut at, &[0xa0, 0x02, 0, 0, 0, 0])
        || length_prefixed_ascii(ctx, data, &mut at)? != Some("CATIA")
        || !fixed(&mut at, &[0x9f])
        || !fixed(&mut at, &[0xa0, 0x02, 0, 0, 0, 0])
    {
        return Ok(None);
    }
    let target_offset = at;
    let Some(target) = length_prefixed_ascii(ctx, data, &mut at)? else {
        return Ok(None);
    };
    Ok(
        (data.get(at) == Some(&0x9f) && is_catia_document_name(target))
            .then_some((target_offset, target)),
    )
}

/// Tag byte plus one-byte length that precede a length-prefixed ASCII string.
const LENGTH_PREFIXED_ASCII_HEADER: Option<NonZeroU32> = NonZeroU32::new(2);

fn length_prefixed_ascii<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    at: &mut usize,
) -> Result<Option<&'a str>, CodecError> {
    let (Some(&0x34), Some(&length)) = (data.get(*at), data.get(*at + 1)) else {
        return Ok(None);
    };
    let start = *at + 2;
    let Some(value) = data.get(start..start + usize::from(length)) else {
        return Ok(None);
    };
    *at = start + usize::from(length);
    if !ctx.is_ascii(value, "catia_length_prefixed_ascii")? {
        return Ok(None);
    }
    Ok(ctx
        .validate_utf8(value, "catia_length_prefixed_ascii")?
        .ok())
}

fn is_catia_document_name(value: &str) -> bool {
    for extension in [".catpart", ".catproduct", ".catshape", ".cgr"] {
        let suffix = value
            .len()
            .checked_sub(extension.len())
            .and_then(|start| value.get(start..));
        if let Some(suffix) = suffix {
            if suffix.eq_ignore_ascii_case(extension) {
                return true;
            }
        }
    }
    false
}

fn parse_last_save_version(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<LastSaveVersion>, CodecError> {
    let number = |open: &[u8], close: &[u8]| -> Result<Option<u16>, CodecError> {
        let Some(value) = tagged_ascii(ctx, data, open, close)? else {
            return Ok(None);
        };
        Ok(ctx.parse_text(value, "catia_last_save_number")?.ok())
    };
    let Some(version) = number(b"<Version>", b"/<Version>")? else {
        return Ok(None);
    };
    let Some(release) = number(b"<Release>", b"/<Release>")? else {
        return Ok(None);
    };
    let Some(service_pack) = number(b"<ServicePack>", b"/<ServicePack>")? else {
        return Ok(None);
    };
    let Some(hot_fix) = number(b"<HotFix>", b"/<HotFix>")? else {
        return Ok(None);
    };
    let Some(build_date) = tagged_ascii(ctx, data, b"<BuildDate>", b"/<BuildDate>")? else {
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

fn tagged_ascii<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    open: &[u8],
    close: &[u8],
) -> Result<Option<&'a str>, CodecError> {
    let Some(open_start) = ctx.find_bytes(data, open, "catia_version_tag_scan")? else {
        return Ok(None);
    };
    let start = open_start + open.len();
    let Some(end) = ctx.find_bytes_from(data, close, start, "catia_version_tag_scan")? else {
        return Ok(None);
    };
    let value = &data[start..end];
    if !ctx.is_ascii(value, "catia_version_tag_text")? {
        return Ok(None);
    }
    Ok(ctx.validate_utf8(value, "catia_version_tag_text")?.ok())
}

/// The end and frame dimensions of the complete JPEG starting at `start`.
/// Each marker step and each entropy-coded byte is charged as it is read.
fn jpeg_extent(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
) -> Result<Option<(usize, u16, u16, u8)>, CodecError> {
    const OPERATION: &str = "catia_jpeg_marker_walk";
    if data.get(start..start + 2) != Some(&[0xff, 0xd8]) {
        return Ok(None);
    }
    let mut at = start + 2;
    let mut frame = None;
    let mut in_entropy = false;
    while at + 1 < data.len() {
        ctx.charge_work(1, OPERATION)?;
        if data[at] != 0xff {
            if !in_entropy {
                return Ok(None);
            }
            // Entropy-coded bytes run to the next marker prefix.
            let skipped = ctx.position_by(&data[at..], |byte| Ok(*byte == 0xff), OPERATION)?;
            let Some(skipped) = skipped else {
                return Ok(None);
            };
            at += skipped;
            continue;
        }
        at += ctx
            .position_by(&data[at..], |byte| Ok(*byte != 0xff), OPERATION)?
            .unwrap_or(data.len() - at);
        let Some(&marker) = data.get(at) else {
            return Ok(None);
        };
        at += 1;
        if in_entropy && marker == 0x00 {
            continue;
        }
        if marker == 0xd9 {
            let Some((width, height, components)) = frame else {
                return Ok(None);
            };
            return Ok(Some((at, width, height, components)));
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        let Some(length) = View::u16_be_at(data, at).map(usize::from) else {
            return Ok(None);
        };
        if length < 2 {
            return Ok(None);
        }
        let payload = at + 2;
        let Some(end) = at.checked_add(length).filter(|end| *end <= data.len()) else {
            return Ok(None);
        };
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let (Some(width), Some(height), Some(&components)) = (
                View::u16_be_at(data, payload + 3),
                View::u16_be_at(data, payload + 1),
                data.get(payload + 5),
            ) else {
                return Ok(None);
            };
            let expected_length = 3 * usize::from(components) + 8;
            if length < 8
                || width == 0
                || height == 0
                || components == 0
                || length != expected_length
                || frame.is_some()
            {
                return Ok(None);
            }
            frame = Some((width, height, components));
        }
        in_entropy = marker == 0xda;
        at = end;
    }
    Ok(None)
}

/// Locate the coherent E5 record stream in the outer-body preamble or a FINJPL segment.
///
/// The candidate range is the complete preamble or complete FINJPL segment. A
/// candidate must contain at least ten stride-valid records. The preamble wins
/// when coherent; otherwise the segment with the largest valid walk wins, with
/// storage type `0x0000_008e` breaking ties. An unresolved tie rejects E5
/// selection.
#[cfg(test)]
pub(crate) fn e5_record_stream(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<Range<usize>>, CodecError> {
    let Some(body) = outer_body_range(data) else {
        return Ok(None);
    };
    let segments = finjpl_segments(ctx, &body)?;
    select_e5_record_stream(ctx, data, body.range(), &segments)
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
pub(crate) fn outer_preamble_range(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<Range<usize>>, CodecError> {
    let Some(body) = outer_body_range(data).or_else(|| {
        (data.starts_with(OUTER_MAGIC)
            && View::u32_be_at(data, outer_hdr::DIRECTORY_OFFSET) == Some(0)
            && View::u32_be_at(data, outer_hdr::DIRECTORY_LENGTH) == Some(0))
        .then(|| BodyExtent::tail_from(data, outer_hdr::FILL_FF))
        .flatten()
    }) else {
        return Ok(None);
    };
    let range = body.range();
    let end = ctx
        .find_bytes(body.bytes(), FINJPL_MARKER, "catia_outer_preamble_scan")?
        .map_or(range.end, |relative| range.start + relative);
    Ok(Some(range.start..end))
}

fn select_e5_record_stream(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    body: Range<usize>,
    segments: &[FinjplSegment],
) -> Result<Option<Range<usize>>, CodecError> {
    let Some(preamble) = outer_preamble_range(ctx, data)? else {
        return Ok(None);
    };
    if coherent_e5_record_count(ctx, &data[preamble.clone()])? >= 10 {
        return Ok(Some(preamble));
    }
    let mut best: Option<(usize, bool, Range<usize>)> = None;
    let mut tied = false;
    for segment in ctx.admit_iter(segments, "catia_e5_segment_scan")? {
        let range = segment.range.clone();
        if range.start < body.start || range.end > body.end {
            continue;
        }
        let count = coherent_e5_record_count(ctx, &data[range.clone()])?;
        if count < 10 {
            continue;
        }
        let preferred = segment.type_word == 0x0000_008e;
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
    Ok(if tied {
        None
    } else {
        best.map(|(_, _, range)| range)
    })
}

/// Count the longest declared-stride E5 walk in a bounded byte region.
///
/// A stream may place the unframed `05 08 01` coordinate roster between E5
/// records. No other gap is part of the walk. One marker search admits the
/// region; a marker that a walk already consumed starts no new walk, and each
/// walk charges its own frame and row steps.
fn coherent_e5_record_count(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<usize, CodecError> {
    let mut best = 0;
    let mut search = 0;
    for start in ctx.find_bytes_iter(data, E5_MARKER, "catia_e5_marker_scan")? {
        if start < search {
            continue;
        }
        let (count, consumed) = e5_record_walk_count(ctx, data, start)?;
        best = best.max(count);
        search = consumed.max(start + 1);
    }
    Ok(best)
}

/// Walks adjacent E5 frames from `start`, crossing vertex-row runs between
/// them, and returns the frame count and the offset after the last step.
fn e5_record_walk_count(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
) -> Result<(usize, usize), CodecError> {
    const OPERATION: &str = "catia_e5_stride_walk";
    let mut count = 0;
    let mut position = start;
    let mut consumed = start;
    loop {
        ctx.charge_work(1, OPERATION)?;
        let Some(frame) = E5Frame::at(data, position) else {
            break;
        };
        count += 1;
        position = frame.end();
        consumed = position;
        if e5_marker_at(data, position) {
            continue;
        }
        let mut next = position;
        while vertex_row_at(data, next) {
            ctx.charge_work(1, OPERATION)?;
            next += VERTEX_ROW_BYTES;
        }
        if next == position {
            break;
        }
        consumed = next;
        if !e5_marker_at(data, next) {
            break;
        }
        position = next;
    }
    Ok((count, consumed))
}

fn e5_marker_at(data: &[u8], position: usize) -> bool {
    data.get(position..)
        .is_some_and(|rest| rest.starts_with(E5_MARKER))
}

/// One unframed `05 08 01` coordinate row.
const VERTEX_ROW_BYTES: usize = 15;

fn vertex_row_at(data: &[u8], position: usize) -> bool {
    data.get(position..)
        .is_some_and(|rest| rest.len() >= VERTEX_ROW_BYTES && rest.starts_with(VERTEX_MARKER))
}

/// Standard-nested BREP-spine markers used for variant identification.
const EDGE_DELIMITER: &[u8; 8] = &[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
const VERTEX_MARKER: &[u8; 3] = &[0x05, 0x08, 0x01];

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
    fn logical_length(&self, ctx: &DecodeContext<'_>) -> Result<u64, CodecError> {
        ctx.fold(
            &self.extents,
            0_u64,
            |length, extent| {
                length
                    .checked_add(u64::from(extent.phys_len))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("catia_logical_stream_length", u64::MAX, u64::MAX)
                    })
            },
            "catia_logical_stream_length",
        )
    }

    /// Whether the descriptor names the stream `name`.
    fn is_named(&self, name: &'static str) -> bool {
        self.name == name
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
    /// Selected coherent E5 stream in the root image.
    pub(crate) e5_record_range: Option<Range<usize>>,
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
    // Each distinct extent list is one source; the seen lists are scratch.
    let mut seen_storage = ctx.reserve_scoped(0, "catia_record_source_keys")?;
    let mut seen = HashSet::<Vec<(usize, usize)>>::new();
    let mut add_directory = |sources: &mut Vec<Vec<SourceExtent>>,
                             directory: &InnerDir|
     -> Result<(), CodecError> {
        for descriptor in
            ctx.admit_iter(&directory.descriptors, "catia_record_source_descriptors")?
        {
            let mut source_storage = ctx.reserve_scoped(0, "catia_record_source_extents")?;
            let mut source = Vec::new();
            for extent in ctx.admit_iter(&descriptor.extents, "catia_record_source_extent_scan")? {
                let Some(start) = directory.inner.checked_add(index_from_u32(extent.phys_off))
                else {
                    continue;
                };
                let Some(end) = start.checked_add(index_from_u32(extent.phys_len)) else {
                    continue;
                };
                // A descriptor extent the image does not hold states no record
                // source. The scanner reads every byte of an extent it accepts,
                // so it never receives a shortened one.
                if let Some(extent) = SourceExtent::within(&scan.data, start, end) {
                    ctx.push_scoped_vec(
                        &mut source_storage,
                        &mut source,
                        extent,
                        "catia_record_source_extents",
                    )?;
                }
            }
            if source.is_empty() {
                continue;
            }
            let key = seen_storage.with_storage(|| {
                ctx.collect_vec(
                    source.iter().map(|extent| {
                        let range = extent.range();
                        (range.start, range.end)
                    }),
                    "catia_record_source_keys",
                )
            })?;
            if seen_storage
                .with_storage(|| ctx.insert_hash_set(&mut seen, key, "catia_record_source_keys"))?
            {
                source_storage.commit()?;
                ctx.push_vec(sources, source, "catia_record_sources")?;
            }
        }
        Ok(())
    };

    if let Some(outer) = scan.outer.as_ref() {
        add_directory(&mut sources, outer)?;
    } else {
        let outer_end = match scan.inner.as_ref() {
            Some(directory) => directory.inner,
            None => outer_stream_directory_range(ctx, &scan.data)?
                .map_or(scan.data.len(), |range| range.start),
        };
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

/// Independent logical sources and their live temporary storage.
pub(crate) struct LogicalRecordStreams<'storage> {
    pub(crate) streams: Vec<Vec<u8>>,
    _stream_storage: Vec<ScopedReservation<'storage>>,
    _storage: ScopedReservation<'storage>,
}

/// Reconstruct catalogued logical streams without joining descriptor namespaces.
/// A container without a directory has one source: its bounded outer preamble.
pub(crate) fn logical_record_streams<'storage>(
    ctx: &'storage DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<LogicalRecordStreams<'storage>, CodecError> {
    let mut streams = Vec::new();
    let mut stream_storage = Vec::new();
    let mut scratch = ctx.reserve_scoped(0, "catia_logical_record_streams")?;
    for directory in [scan.outer.as_ref(), scan.inner.as_ref()]
        .into_iter()
        .flatten()
    {
        for descriptor in
            ctx.admit_iter(&directory.descriptors, "catia_logical_stream_descriptors")?
        {
            let (stream, storage) =
                reconstruct_logical_stream(ctx, &scan.data, descriptor, directory.inner)?;
            if !stream.is_empty() {
                ctx.push_scoped_vec(
                    &mut scratch,
                    &mut streams,
                    stream,
                    "catia_logical_record_streams",
                )?;
                ctx.push_scoped_vec(
                    &mut scratch,
                    &mut stream_storage,
                    storage,
                    "catia_logical_stream_reservations",
                )?;
            }
        }
    }
    if streams.is_empty() {
        if let Some(range) = outer_preamble_range(ctx, &scan.data)? {
            scratch.with_storage(|| {
                let stream = ctx.copy_slice(&scan.data[range], "catia_outer_preamble_stream")?;
                ctx.push_vec(&mut streams, stream, "catia_logical_record_streams")
            })?;
        }
    }
    Ok(LogicalRecordStreams {
        streams,
        _stream_storage: stream_storage,
        _storage: scratch,
    })
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
        ctx.charge_work(1, "catia_fbb_scan")?;
        if is_fbb_row(&body[position..]) {
            let start = position;
            while position + fbb_row::LEN <= body.len() && is_fbb_row(&body[position..]) {
                ctx.charge_work(1, "catia_fbb_scan")?;
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

fn count_subslice(
    ctx: &DecodeContext<'_>,
    haystack: &[u8],
    needle: &[u8],
) -> Result<usize, CodecError> {
    Ok(ctx
        .find_bytes_iter(haystack, needle, "catia_census_marker_scan")?
        .count())
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
    let Some(inner) = ctx.find_bytes_from(
        data,
        OUTER_MAGIC,
        OUTER_MAGIC.len(),
        "catia_nested_magic_scan",
    )?
    else {
        return Ok(None);
    };
    let Some((inner, dir_offset, b)) = (|| {
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
pub(crate) fn outer_stream_directory_range(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<Range<usize>>, CodecError> {
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
    Ok(
        directory_region_has_descriptor(ctx, data, 0, dir_offset, dir_length)?
            .then_some(dir_offset..dir_end),
    )
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
        ctx.charge_work(1, "catia_directory_candidate_scan")?;
        let Some(k) = View::u32_be_at(dirbuf, o).and_then(|value| usize::try_from(value).ok())
        else {
            break;
        };
        let extents_end = k
            .checked_mul(extent::LEN)
            .and_then(|extent_bytes| o.checked_add(4)?.checked_add(extent_bytes));
        if k != 0 && extents_end.is_some_and(|end| end <= dirbuf.len()) {
            let mut candidate_storage =
                ctx.reserve_scoped(0, "catia_directory_extent_candidate")?;
            if let Some((extents, cum)) = candidate_storage
                .with_storage(|| parse_extents(ctx, dirbuf, o, k, physical_base, file_len))?
            {
                if cum > 0 && o >= stream_desc::EXTENT_COUNT {
                    let ds = o - stream_desc::EXTENT_COUNT;
                    let logical_length =
                        View::u32_be_at(dirbuf, ds + stream_desc::LOGICAL_STREAM_LENGTH)
                            .unwrap_or(0);
                    if index_from_u32(logical_length) == cum {
                        let name = descriptor_name(ctx, dirbuf, ds)?;
                        candidate_storage.commit()?;
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
    ctx: &DecodeContext<'_>,
    data: &[u8],
    physical_base: usize,
    dir_offset: usize,
    dir_length: usize,
) -> Result<bool, CodecError> {
    let Some(dir_end) = dir_offset.checked_add(dir_length) else {
        return Ok(false);
    };
    let Some(magic_end) = dir_offset.checked_add(DIR_MAGIC.len()) else {
        return Ok(false);
    };
    if dir_length == 0 || dir_end > data.len() || data.get(dir_offset..magic_end) != Some(DIR_MAGIC)
    {
        return Ok(false);
    }
    let dirbuf = &data[dir_offset..dir_end];
    if dirbuf.len() < 4 {
        return Ok(false);
    }
    for o in 0..=dirbuf.len() - 4 {
        ctx.charge_work(1, "catia_directory_candidate_scan")?;
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
        let Some(cum) = validate_extents(ctx, dirbuf, o, k, physical_base, data.len())? else {
            continue;
        };
        let ds = o - stream_desc::EXTENT_COUNT;
        if cum > 0
            && View::u32_be_at(dirbuf, ds + stream_desc::LOGICAL_STREAM_LENGTH)
                .is_some_and(|length| index_from_u32(length) == cum)
        {
            return Ok(true);
        }
    }
    Ok(false)
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
    let Some(cum) = validate_extents(ctx, dirbuf, o, k, physical_base, file_len)? else {
        return Ok(None);
    };
    let mut extents = ctx.vector_storage(k, "catia_directory_extents")?;
    for i in ctx.admit_iter(0..k, "catia_extent_copy")? {
        let Some((extent, _, _)) = read_extent_fields(dirbuf, o, i) else {
            return Ok(None);
        };
        ctx.push_vec(&mut extents, extent, "catia_directory_extents")?;
    }
    Ok(Some((extents, cum)))
}

/// Validates extents until the first that fails, charging each one read.
fn validate_extents(
    ctx: &DecodeContext<'_>,
    dirbuf: &[u8],
    o: usize,
    k: usize,
    physical_base: usize,
    file_len: usize,
) -> Result<Option<usize>, CodecError> {
    let mut cum: usize = 0;
    let valid = ctx.all_by(
        0..k,
        |i| {
            let Some((extent, log_len, log_off)) = read_extent_fields(dirbuf, o, i) else {
                return Ok(false);
            };
            let phys_end = physical_base
                .checked_add(index_from_u32(extent.phys_off))
                .and_then(|start| start.checked_add(index_from_u32(extent.phys_len)));
            if extent.phys_len == 0
                || phys_end.is_none_or(|end| end > file_len)
                || index_from_u32(log_off) != cum
                || log_len != extent.phys_len
            {
                return Ok(false);
            }
            let Some(next) = cum.checked_add(index_from_u32(log_len)) else {
                return Ok(false);
            };
            cum = next;
            Ok(true)
        },
        "catia_extent_validation",
    )?;
    Ok(valid.then_some(cum))
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
                ctx.charge_work(1, "catia_container_iteration")?;
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
///
/// The bytes are held under the returned scoped reservation; a caller that
/// keeps them commits it.
fn reconstruct_logical_stream<'storage>(
    ctx: &'storage DecodeContext<'_>,
    data: &[u8],
    descriptor: &Descriptor,
    inner: usize,
) -> Result<(Vec<u8>, ScopedReservation<'storage>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "catia_logical_stream_bytes")?;
    let mut out = Vec::new();
    let length = usize::try_from(descriptor.logical_length(ctx)?).unwrap_or(0);
    if length <= data.len() {
        storage.with_storage(|| {
            ctx.reserve_capacity(&mut out, length, "catia_logical_stream_bytes")
        })?;
    }
    for extent in ctx.admit_iter(&descriptor.extents, "catia_logical_stream_extents")? {
        let bytes = inner
            .checked_add(index_from_u32(extent.phys_off))
            .and_then(|start| data.get(start..start.checked_add(index_from_u32(extent.phys_len))?));
        // An extent the image does not hold states no logical stream.
        let Some(bytes) = bytes else {
            return Ok((Vec::new(), storage));
        };
        storage.with_storage(|| {
            ctx.extend_retained_bytes(&mut out, bytes, "catia_logical_stream_copy")
        })?;
    }
    Ok((out, storage))
}

/// Decode model-container declarations whose UUIDs select named outer streams.
pub(crate) fn outer_container_declarations(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    outer: &InnerDir,
) -> Result<Vec<OuterContainerDeclaration>, CodecError> {
    let mut data_descriptor = None;
    let mut descriptors = outer.descriptors.iter();
    while let Some(descriptor) =
        ctx.next_charged(&mut descriptors, "catia_container_data_stream_scan")?
    {
        if descriptor.is_named("Data") {
            if data_descriptor.is_some() {
                return Ok(Vec::new());
            }
            data_descriptor = Some(descriptor);
        }
    }
    let Some(data_descriptor) = data_descriptor else {
        return Ok(Vec::new());
    };
    let (logical, _storage) = reconstruct_logical_stream(ctx, data, data_descriptor, outer.inner)?;
    parse_outer_container_declarations(ctx, &logical, &outer.descriptors)
}

/// At each start, the largest ends belonging to two distinct declarations.
struct DeclaredExtent {
    start: u64,
    owners: [Option<(u64, usize)>; 2],
}

/// One borrowed declaration index with live temporary interval storage.
pub(crate) struct OuterContainerIndex<'a, 'storage> {
    declarations: &'a [OuterContainerDeclaration],
    extents: Vec<DeclaredExtent>,
    _storage: ScopedReservation<'storage>,
}

/// Index declaration names once, then key their physical extents by start.
pub(crate) fn outer_container_extent_index<'a, 'storage>(
    ctx: &'storage DecodeContext<'_>,
    outer: &InnerDir,
    declarations: &'a [OuterContainerDeclaration],
) -> Result<OuterContainerIndex<'a, 'storage>, CodecError> {
    const OPERATION: &str = "catia_outer_container_index";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let ordered = scratch.with_storage(|| {
        let mut by_name = HashMap::<&str, Vec<usize>>::new();
        for (index, declaration) in ctx.admit_iter(declarations, OPERATION)?.enumerate() {
            ctx.push_hash_group(
                &mut by_name,
                declaration.stream_name.as_str(),
                index,
                OPERATION,
                OPERATION,
            )?;
        }
        let mut ordered = BTreeSet::new();
        for descriptor in ctx.admit_iter(&outer.descriptors, OPERATION)? {
            let Some(owners) = ctx.get_hash_map(&by_name, descriptor.name.as_str(), OPERATION)?
            else {
                continue;
            };
            for extent in ctx.admit_iter(&descriptor.extents, OPERATION)? {
                let Some(start) =
                    u64_from_index(outer.inner).checked_add(u64::from(extent.phys_off))
                else {
                    continue;
                };
                let Some(end) = start.checked_add(u64::from(extent.phys_len)) else {
                    continue;
                };
                for &owner in ctx.admit_iter(owners, OPERATION)? {
                    ctx.insert_btree_set(&mut ordered, (start, end, owner), OPERATION)?;
                }
            }
        }
        Ok::<_, CodecError>(ordered)
    })?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut owners = [None::<(u64, usize)>; 2];
    let extents = storage.with_storage(|| {
        ctx.collect_vec(
            ordered.into_iter().map(|(start, end, declaration)| {
                if let Some(slot) = owners
                    .iter_mut()
                    .find(|slot| slot.is_some_and(|(_, owner)| owner == declaration))
                {
                    if let Some((previous_end, _)) = *slot {
                        *slot = Some((previous_end.max(end), declaration));
                    }
                } else if owners[0].is_none() {
                    owners[0] = Some((end, declaration));
                } else if owners[1].is_none_or(|(previous_end, _)| end > previous_end) {
                    owners[1] = Some((end, declaration));
                }
                if owners[1]
                    .is_some_and(|(second, _)| owners[0].is_none_or(|(first, _)| second > first))
                {
                    owners.swap(0, 1);
                }
                DeclaredExtent { start, owners }
            }),
            OPERATION,
        )
    })?;
    Ok(OuterContainerIndex {
        declarations,
        extents,
        _storage: storage,
    })
}

/// Select the unique declaration whose single physical extent contains the range.
/// The two prefix owners distinguish unique containment from overlap.
pub(crate) fn outer_container_for_extent<'a>(
    ctx: &DecodeContext<'_>,
    index: &OuterContainerIndex<'a, '_>,
    byte_offset: u64,
    byte_len: u64,
) -> Result<Option<&'a OuterContainerDeclaration>, CodecError> {
    const OPERATION: &str = "catia_outer_container_extent_query";
    let Some(byte_end) = byte_offset.checked_add(byte_len) else {
        return Ok(None);
    };
    let position = ctx.partition_point(
        &index.extents,
        |extent| Ok(extent.start <= byte_offset),
        OPERATION,
    )?;
    let Some(extent) = position
        .checked_sub(1)
        .and_then(|position| index.extents.get(position))
    else {
        return Ok(None);
    };
    let Some((end, owner)) = extent.owners[0] else {
        return Ok(None);
    };
    if end < byte_end || extent.owners[1].is_some_and(|(end, _)| end >= byte_end) {
        return Ok(None);
    }
    Ok(Some(&index.declarations[owner]))
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
    // The terminal positions, the descriptor names and the selected stream
    // names are scratch indexes built once.
    let mut scratch = ctx.reserve_scoped(0, "catia_container_declaration_scratch")?;
    let mut terminals = Vec::new();
    for terminal in ctx.find_bytes_iter(data, TERMINAL, "catia_container_terminal_scan")? {
        ctx.push_scoped_vec(
            &mut scratch,
            &mut terminals,
            terminal,
            "catia_container_terminals",
        )?;
    }
    let mut descriptor_names = HashSet::new();
    for descriptor in ctx.admit_iter(descriptors, "catia_container_descriptor_name_scan")? {
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut descriptor_names,
                descriptor.name.as_str(),
                "catia_container_descriptor_lookup",
            )
        })?;
    }
    let mut selected_streams = HashSet::new();
    let mut output_storage = ctx.reserve_scoped(0, "catia_container_declarations")?;
    let mut starts = 0..data.len() - 64;
    while let Some(start) = ctx.next_charged(&mut starts, "catia_container_declaration_scan")? {
        if data.get(start + 8..start + 12) != Some(HEADER)
            || data.get(start + 16..start + 24) != Some(PREFIX)
            || data.get(start + 32..start + 36) != Some(CLASS_BLOCK)
        {
            continue;
        }
        let strings_start = start + 40;
        let first_terminal = ctx.partition_point(
            &terminals,
            |terminal| Ok(*terminal < strings_start),
            "catia_container_terminal_lookup",
        )?;
        let Some(&terminal) = terminals.get(first_terminal) else {
            continue;
        };
        let Some((class_name, base_class)) =
            declaration_class_pair(ctx, &data[strings_start..terminal])?
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
        let (canonical_stream_name, _canonical_storage) = ctx.format_scoped(
            format_args!("{first:x}_{middle:08x}_{last:x}"),
            "catia_container_stream_name",
        )?;
        let (prefixed_stream_name, _prefixed_storage) = ctx.format_scoped(
            format_args!("_{canonical_stream_name}"),
            "catia_container_stream_name",
        )?;
        let stream_name = match (
            ctx.contains_hash_set(
                &descriptor_names,
                canonical_stream_name.as_str(),
                "catia_container_descriptor_lookup",
            )?,
            ctx.contains_hash_set(
                &descriptor_names,
                prefixed_stream_name.as_str(),
                "catia_container_descriptor_lookup",
            )?,
        ) {
            (true, false) => canonical_stream_name.as_str(),
            (false, true) => prefixed_stream_name.as_str(),
            (false, false) | (true, true) => continue,
        };
        let Some(ordinal) = View::u32_le_at(data, start + 12) else {
            continue;
        };
        // Two declarations that select one stream select neither.
        if ctx.contains_hash_set(
            &selected_streams,
            stream_name,
            "catia_container_selected_streams",
        )? {
            return Ok(Vec::new());
        }
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut selected_streams,
                ctx.copy_retained_text(stream_name, "catia_container_selected_streams")?,
                "catia_container_selected_streams",
            )
        })?;
        output_storage.with_storage(|| {
            let class_name = ctx.copy_retained_text(class_name, "catia_container_class_name")?;
            let base_class = ctx.copy_retained_text(base_class, "catia_container_base_class")?;
            let stream_name = ctx.copy_retained_text(stream_name, "catia_container_stream_name")?;
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
            )
        })?;
    }
    output_storage.commit()?;
    Ok(declarations)
}

/// The two NUL-terminated graphic-ASCII class names before a declaration's
/// terminal, followed only by NUL padding. Each byte is charged as it is read.
fn declaration_class_pair<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
) -> Result<Option<(&'a str, &'a str)>, CodecError> {
    const OPERATION: &str = "catia_container_class_scan";
    let name = |bytes: &'a [u8]| -> Result<Option<(&'a str, usize)>, CodecError> {
        let Some(end) = ctx.position_by(bytes, |byte| Ok(!byte.is_ascii_graphic()), OPERATION)?
        else {
            return Ok(None);
        };
        if end == 0 || bytes[end] != 0 {
            return Ok(None);
        }
        Ok(ctx
            .validate_utf8(&bytes[..end], OPERATION)?
            .ok()
            .map(|text| (text, end)))
    };
    let Some((first, first_end)) = name(data)? else {
        return Ok(None);
    };
    let Some((second, second_len)) = name(&data[first_end + 1..])? else {
        return Ok(None);
    };
    let padding = &data[first_end + 1 + second_len..];
    if !ctx.all_by(padding, |byte| Ok(*byte == 0), OPERATION)? {
        return Ok(None);
    }
    Ok(Some((first, second)))
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
    let Some(main) = unique_largest_descriptor(ctx, &dir.descriptors, "MainDataStream")? else {
        return Ok(None);
    };
    let Some(surf) = unique_largest_descriptor(ctx, &dir.descriptors, "SurfacicReps")? else {
        return Ok(None);
    };
    let (mut out, mut output_storage) = reconstruct_logical_stream(ctx, data, main, dir.inner)?;
    let (surface, _storage) = reconstruct_logical_stream(ctx, data, surf, dir.inner)?;
    output_storage.with_storage(|| {
        ctx.extend_retained_bytes(&mut out, &surface, "catia_brep_surface_bytes")
    })?;
    output_storage.commit()?;
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
    let Some(main) = unique_largest_descriptor(ctx, &dir.descriptors, "MainDataStream")? else {
        return Ok(None);
    };
    let (stream, storage) = reconstruct_logical_stream(ctx, data, main, dir.inner)?;
    storage.commit()?;
    Ok(Some(stream))
}

/// The descriptor named `name` with the uniquely largest logical length.
fn unique_largest_descriptor<'a>(
    ctx: &DecodeContext<'_>,
    descriptors: &'a [Descriptor],
    name: &'static str,
) -> Result<Option<&'a Descriptor>, CodecError> {
    let mut selected = None;
    let mut selected_length = 0;
    let mut equal_count = 0;
    for descriptor in ctx.admit_iter(descriptors, "catia_largest_descriptor_scan")? {
        if !descriptor.is_named(name) {
            continue;
        }
        let logical_length = descriptor.logical_length(ctx)?;
        if selected.is_none() || logical_length > selected_length {
            selected = Some(descriptor);
            selected_length = logical_length;
            equal_count = 1;
        } else if logical_length == selected_length {
            equal_count += 1;
        }
    }
    Ok((equal_count == 1).then_some(selected).flatten())
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
    if data.get(..OUTER_MAGIC.len()) != Some(OUTER_MAGIC.as_slice()) {
        return Err(CodecError::WrongFormat(
            "missing V5_CFV2 container magic".to_owned(),
        ));
    }
    let outer_dir_offset = View::u32_be_at(&data, outer_hdr::DIRECTORY_OFFSET)
        .ok_or_else(|| CodecError::malformed("truncated outer directory offset"))?;
    let outer_dir_length = View::u32_be_at(&data, outer_hdr::DIRECTORY_LENGTH)
        .ok_or_else(|| CodecError::malformed("truncated outer directory length"))?;

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

    let a9_records = match outer_preamble_range(ctx, &data)? {
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
            .map(|body| count_subslice(ctx, body.bytes(), E5_MARKER))
            .transpose()?
            .unwrap_or(0),
        ..Default::default()
    };
    if let Some(b) = main_data_stream.as_deref() {
        // The census keeps only the counts, so the runs are scratch.
        let mut runs_storage = ctx.reserve_scoped(0, "catia_fbb_run_census")?;
        let fbb_ranges = runs_storage.with_storage(|| fbb_run_ranges(ctx, b))?;
        census.fbb_runs = fbb_ranges.len();
        census.fbb_face_rows = ctx.fold(
            &fbb_ranges,
            0_usize,
            |rows, range| Ok(rows + (range.end - range.start) / fbb_row::LEN),
            "catia_fbb_run_census",
        )?;
        census.edge_delimiters = count_subslice(ctx, b, EDGE_DELIMITER)?;
        census.vertex_markers = count_subslice(ctx, b, VERTEX_MARKER)?;
    }

    let e5_record_range = match outer_body.as_ref() {
        Some(body) => select_e5_record_stream(ctx, &data, body.range(), &finjpl_segments)?,
        None => None,
    };
    let variant = identify_variant(
        ctx,
        inner.as_ref(),
        brep.as_deref(),
        main_data_stream.as_deref(),
        &census,
        e5_record_range.is_some(),
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
        e5_record_range,
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
    let mut declaration_storage = ctx.reserve_scoped(0, "catia_summary_declaration_index")?;
    let declarations = declaration_storage.with_storage(|| {
        let mut declarations = HashMap::new();
        for declaration in ctx.admit_iter(
            &scan.outer_container_declarations,
            "catia_summary_declaration_index",
        )? {
            ctx.entry_hash_map(
                &mut declarations,
                declaration.stream_name.as_str(),
                "catia_summary_declaration_index",
            )?
            .or_insert(declaration);
        }
        Ok::<_, CodecError>(declarations)
    })?;
    let mut entries = Vec::new();

    for (directory, dir) in [
        ("outer", scan.outer.as_ref()),
        ("inner", scan.inner.as_ref()),
    ] {
        let Some(dir) = dir else { continue };
        for d in ctx.admit_iter(&dir.descriptors, "catia_summary_descriptor_scan")? {
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
                if let Some(declaration) = ctx.get_hash_map(
                    &declarations,
                    d.name.as_str(),
                    "catia_summary_declaration_lookup",
                )? {
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
            let phys = d.logical_length(ctx)?;
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
    for (index, preview) in ctx
        .admit_iter(&scan.previews, "catia_summary_preview_scan")?
        .enumerate()
    {
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
                    stored: Some(u64_from_index(preview.range.end - preview.range.start)),
                    expanded: None,
                },
                attributes,
            },
            "catia_summary_entries",
        )?;
    }
    for reference in ctx.admit_iter(&scan.external_references, "catia_summary_reference_scan")? {
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
            LENGTH_PREFIXED_ASCII_HEADER.ok_or_else(|| {
                CodecError::InvalidInput("CATIA ASCII framing must be nonzero".into())
            })?,
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
    for (index, segment) in ctx
        .admit_iter(&scan.finjpl_segments, "catia_summary_segment_scan")?
        .enumerate()
    {
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
                    u64_from_index(segment.range.end - segment.range.start),
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
