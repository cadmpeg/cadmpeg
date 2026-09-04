//! Generic CATIA record-walking and wire-shape record decoding.
//!
//! The family-independent scan layer: length-closed A/B record framing
//! (`consolidated_records` and its `a_family_frames`/`b_family_frames` views),
//! the `05 08 01` vertex-coordinate scanner (`scan_vertex_records`), and the
//! degree-5 UV jet decoder (`parse_consolidated_pcurve`). Nothing here depends
//! on a `families` module; the family decoders consume it downward.
//!
//! The `Consolidated*` names are retained from this code's original home in
//! `families::consolidated::records`; a rename cascades across ~40 call sites
//! and several `native` field paths, so the names carry naming debt here.

use std::{collections::HashSet, ops::Range};

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::knots_strictly_increasing;
use cadmpeg_ir::math::Point3;

use crate::layout::a_family_frame as a_frame;
use crate::layout::b_family_frame as b_frame;

use super::bytes::{compact_int, f64_le};

/// One knot of a degree-5 consolidated UV jet.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidatedPcurveSite {
    /// Global parameter at this site.
    pub knot: f64,
    /// UV position.
    pub point: [f64; 2],
    /// UV first derivative.
    pub first_derivatives: [f64; 2],
    /// UV second derivative.
    pub second_derivatives: [f64; 2],
}

/// Degree-5 UV jet stored in an A- or B-family class-`0x20` consolidated record.
#[derive(Debug, Clone)]
pub struct ConsolidatedPcurve {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support-surface identifier.
    pub support_id: u32,
    /// Number of leading extrapolation sites encoded by the array marker.
    pub extrapolation_sites: u32,
    /// Knot-aligned UV jet samples.
    pub sites: Vec<ConsolidatedPcurveSite>,
    /// Native parameter range.
    pub range: [f64; 2],
    /// Bytes following the native range inside the framed record.
    pub tail: Vec<u8>,
}

impl ConsolidatedPcurve {
    pub const DEGREE: u32 = 5;

    pub fn knots(&self) -> Vec<f64> {
        self.sites.iter().map(|site| site.knot).collect()
    }

    pub fn points(&self) -> Vec<[f64; 2]> {
        self.sites.iter().map(|site| site.point).collect()
    }

    pub fn first_derivatives(&self) -> Vec<[f64; 2]> {
        self.sites
            .iter()
            .map(|site| site.first_derivatives)
            .collect()
    }

    pub fn second_derivatives(&self) -> Vec<[f64; 2]> {
        self.sites
            .iter()
            .map(|site| site.second_derivatives)
            .collect()
    }
}

pub(crate) fn parse_consolidated_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
) -> Option<ConsolidatedPcurve> {
    let mut at = payload;
    let support_id = compact_int(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    if degree != 5 || count < 2 {
        return None;
    }
    let extrapolation_sites = match *data.get(at)? {
        0x0c => {
            at += 1;
            0
        }
        0x08 => {
            let encoded = *data.get(at + 1)?;
            if encoded % 4 != 1 {
                return None;
            }
            at += 2;
            u32::from((encoded - 1) / 4)
        }
        _ => return None,
    };
    let knot_bytes = count.checked_mul(8)?;
    if at.checked_add(knot_bytes.checked_add(20)?)? > end {
        return None;
    }
    let read = |at: &mut usize| -> Option<Vec<f64>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read(&mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    at = at.checked_add(1)?;
    if at > end {
        return None;
    }
    let remaining_array_bytes = count.checked_mul(8)?.checked_mul(6)?;
    if at.checked_add(remaining_array_bytes.checked_add(18)?)? > end {
        return None;
    }
    let u = read(&mut at)?;
    let v = read(&mut at)?;
    let du = read(&mut at)?;
    let dv = read(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = read(&mut at)?;
    let ddv = read(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if at > end
        || !matches!(&data[at..end], [0x07] | [0x07, 0x00])
        || !knots_strictly_increasing(&knots)
        || range[0] >= range[1]
        || knots
            .iter()
            .chain(&u)
            .chain(&v)
            .chain(&du)
            .chain(&dv)
            .chain(&ddu)
            .chain(&ddv)
            .chain(&range)
            .any(|x| !x.is_finite())
    {
        return None;
    }
    Some(ConsolidatedPcurve {
        pos,
        support_id,
        extrapolation_sites,
        sites: knots
            .into_iter()
            .zip(u.into_iter().zip(v))
            .zip(du.into_iter().zip(dv))
            .zip(ddu.into_iter().zip(ddv))
            .map(
                |(((knot, (u, v)), (du, dv)), (ddu, ddv))| ConsolidatedPcurveSite {
                    knot,
                    point: [u, v],
                    first_derivatives: [du, dv],
                    second_derivatives: [ddu, ddv],
                },
            )
            .collect(),
        range,
        tail: data[at..end].to_vec(),
    })
}

/// Length-closed A/B-family frame shared by edge-definition and descriptor records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedRawFrame {
    /// Record byte offset.
    pub pos: usize,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct ConsolidatedFrame {
    pub(crate) pos: usize,
    pub(crate) payload: usize,
    pub(crate) end: usize,
    pub(crate) header_token: u32,
}

/// Width-coded consolidated record family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidatedFamily {
    /// U32-length A family (`a5/a6/a7`).
    A,
    /// U8-length B family (`b2/b3/b4`).
    B,
}

/// One length-closed record in a consolidated A/B cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedRecord {
    /// Zero-based logical record-source ordinal supplied by the container.
    pub source_index: usize,
    /// Byte range in the reconstructed logical record source.
    pub source_range: Range<usize>,
    /// Whether the complete frame occupies one physical extent.
    pub physically_contiguous: bool,
    /// Record family.
    pub family: ConsolidatedFamily,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent flag byte (`0x03`, `0x13`, or `0x83`).
    pub flag: u8,
    /// Record class byte.
    pub class: u8,
    /// Little-endian width-coded header token.
    pub header_token: u32,
    /// Complete record byte range.
    pub range: Range<usize>,
    /// Payload byte range.
    pub payload: Range<usize>,
}

/// Return whether a record slice is one logically contiguous frame run.
///
/// A record census may contain independently discovered frames separated by
/// bytes whose grammar is not known. Those frames remain useful for individual
/// typed decoding, but they cannot establish an ordered multi-frame owner
/// relationship. Every semantic window must prove this invariant locally.
pub(crate) fn records_are_contiguous(records: &[ConsolidatedRecord]) -> bool {
    records.windows(2).all(|pair| {
        pair[0].source_index == pair[1].source_index
            && pair[0].source_range.end == pair[1].source_range.start
    })
}

/// Inventory length-closed consolidated A/B records in one bounded source.
///
/// This convenience form is for a byte slice that is already one record
/// source, such as a synthesized stream fixture. Container decode paths use
/// [`consolidated_records_in_sources`] so directory and unrelated-file bytes
/// cannot seed the inventory.
#[must_use]
pub fn consolidated_records(data: &[u8]) -> Vec<ConsolidatedRecord> {
    consolidated_records_in_sources(data, std::iter::once(std::iter::once(0..data.len())))
}

/// Inventory length-closed consolidated A/B records in disjoint physical
/// source extents.
///
/// The record grammar is length-closed, but it does not define a marker that
/// identifies the first record in an arbitrary file image. Callers therefore
/// supply the physical extents that contain record sources. A record that
/// crosses an extent boundary is not a complete record in that source and is
/// withheld. The returned records retain their file-relative byte ranges.
#[must_use]
#[cfg(test)]
pub(crate) fn consolidated_records_in_ranges(
    data: &[u8],
    ranges: impl IntoIterator<Item = Range<usize>>,
) -> Vec<ConsolidatedRecord> {
    consolidated_records_in_sources(data, ranges.into_iter().map(std::iter::once))
}

/// Inventory records in descriptor-scoped logical sources. Physical extents
/// within one source are visited in logical-offset order. A frame that starts
/// immediately after a retained frame and spans an extent boundary is retained
/// as an ordinal-bearing non-contiguous frame; typed payload decoding requires
/// one physical extent.
pub(crate) fn consolidated_records_in_sources<S, R>(
    data: &[u8],
    sources: S,
) -> Vec<ConsolidatedRecord>
where
    S: IntoIterator<Item = R>,
    R: IntoIterator<Item = Range<usize>>,
{
    let mut records = Vec::new();
    for (source_index, ranges) in sources.into_iter().enumerate() {
        let source_ranges = ranges
            .into_iter()
            .filter_map(|range| {
                let start = range.start.min(data.len());
                let end = range.end.min(data.len());
                (start < end).then_some(start..end)
            })
            .collect::<Vec<_>>();
        let mut source_records = Vec::new();
        let mut source_offset = 0usize;
        for range in &source_ranges {
            let start = range.start;
            let end = range.end;
            let mut pos = start;
            while pos < end {
                let Some(mut record) = parse_consolidated_record(data, pos, end) else {
                    pos += 1;
                    continue;
                };
                record.source_index = source_index;
                let Some(source_start) = source_offset.checked_add(record.range.start - start)
                else {
                    return records;
                };
                let Some(source_end) = source_offset.checked_add(record.range.end - start) else {
                    return records;
                };
                record.source_range = source_start..source_end;
                pos = record.range.end;
                source_records.push(record);
            }
            let Some(next_source_offset) = source_offset.checked_add(end - start) else {
                return records;
            };
            source_offset = next_source_offset;
        }
        let mut record_starts = source_records
            .iter()
            .map(|record| record.source_range.start)
            .collect::<HashSet<_>>();
        let mut record_ranges = source_records
            .iter()
            .map(|record| (record.source_range.start, record.source_range.end))
            .collect::<HashSet<_>>();
        loop {
            let mut added = Vec::new();
            let source_ends = source_records
                .iter()
                .map(|record| record.source_range.end)
                .collect::<HashSet<_>>();
            for source_start in source_ends {
                if record_starts.contains(&source_start) {
                    continue;
                }
                let Some(record) = parse_spanning_consolidated_record(
                    data,
                    &source_ranges,
                    source_index,
                    source_start,
                ) else {
                    continue;
                };
                if record_ranges.insert((record.source_range.start, record.source_range.end)) {
                    record_starts.insert(record.source_range.start);
                    added.push(record);
                }
            }
            if added.is_empty() {
                break;
            }
            source_records.extend(added);
            source_records.sort_by_key(|record| record.source_range.start);
        }
        records.extend(source_records);
    }
    records
}

fn parse_spanning_consolidated_record(
    data: &[u8],
    ranges: &[Range<usize>],
    source_index: usize,
    source_start: usize,
) -> Option<ConsolidatedRecord> {
    let source_length = ranges.iter().try_fold(0usize, |length, range| {
        length.checked_add(range.end - range.start)
    })?;
    let source_byte = |offset: usize| {
        let mut logical_start = 0usize;
        for range in ranges {
            let logical_end = logical_start.checked_add(range.end - range.start)?;
            if offset < logical_end {
                return data
                    .get(
                        range
                            .start
                            .checked_add(offset.checked_sub(logical_start)?)?,
                    )
                    .copied();
            }
            logical_start = logical_end;
        }
        None
    };
    let first = source_byte(source_start)?;
    let (family, width, header_len, length) = if let Some(width) = first
        .checked_sub(0xa4)
        .filter(|width| (1..=3).contains(width))
    {
        let length_bytes = [
            source_byte(source_start.checked_add(3)?)?,
            source_byte(source_start.checked_add(4)?)?,
            source_byte(source_start.checked_add(5)?)?,
            source_byte(source_start.checked_add(6)?)?,
        ];
        let length =
            View::u32_le_at(&length_bytes, 0).and_then(|value| usize::try_from(value).ok())?;
        (ConsolidatedFamily::A, width, a_frame::LEN, length)
    } else {
        let width = first
            .checked_sub(0xb1)
            .filter(|width| (1..=3).contains(width))?;
        (
            ConsolidatedFamily::B,
            width,
            b_frame::LEN,
            usize::from(source_byte(
                source_start.checked_add(b_frame::PAYLOAD_LEN)?,
            )?),
        )
    };
    let flag = source_byte(source_start.checked_add(a_frame::FLAG)?)?;
    let class = source_byte(source_start.checked_add(a_frame::CLASS)?)?;
    if ![0x03, 0x13, 0x83].contains(&flag) {
        return None;
    }
    let token_at = source_start.checked_add(header_len)?;
    let payload_start = token_at.checked_add(usize::from(width))?;
    let source_end = payload_start.checked_add(length)?;
    if source_end > source_length {
        return None;
    }
    let mut logical_boundary = 0usize;
    let mut crosses_extent = false;
    for range in ranges.iter().take(ranges.len().saturating_sub(1)) {
        logical_boundary = logical_boundary.checked_add(range.end - range.start)?;
        crosses_extent |= source_start < logical_boundary && logical_boundary < source_end;
    }
    if !crosses_extent {
        return None;
    }
    let header_token = (0..usize::from(width)).try_fold(0u32, |value, relative| {
        Some(value | (u32::from(source_byte(token_at.checked_add(relative)?)?) << (8 * relative)))
    })?;
    let mut logical_start = 0usize;
    let byte_offset = ranges.iter().find_map(|range| {
        let logical_end = logical_start.checked_add(range.end - range.start)?;
        if source_start < logical_end {
            range
                .start
                .checked_add(source_start.checked_sub(logical_start)?)
        } else {
            logical_start = logical_end;
            None
        }
    })?;
    Some(ConsolidatedRecord {
        source_index,
        source_range: source_start..source_end,
        physically_contiguous: false,
        family,
        width,
        flag,
        class,
        header_token,
        range: byte_offset..byte_offset,
        payload: byte_offset..byte_offset,
    })
}

fn parse_consolidated_record(
    data: &[u8],
    pos: usize,
    source_end: usize,
) -> Option<ConsolidatedRecord> {
    let flags = [0x03, 0x13, 0x83];
    let (family, width, token_at, length) = if let Some(width) = data
        .get(pos)
        .and_then(|byte| byte.checked_sub(0xa4))
        .filter(|width| (1..=3).contains(width))
    {
        let length = View::u32_le_at(data, pos.checked_add(a_frame::PAYLOAD_LEN)?)
            .and_then(|value| usize::try_from(value).ok())?;
        (
            ConsolidatedFamily::A,
            width,
            pos.checked_add(a_frame::LEN)?,
            length,
        )
    } else {
        let width = data
            .get(pos)
            .and_then(|byte| byte.checked_sub(0xb1))
            .filter(|width| (1..=3).contains(width))?;
        (
            ConsolidatedFamily::B,
            width,
            pos.checked_add(b_frame::LEN)?,
            usize::from(*data.get(pos.checked_add(b_frame::PAYLOAD_LEN)?)?),
        )
    };
    let flag = *data.get(pos.checked_add(a_frame::FLAG)?)?;
    let class = *data.get(pos.checked_add(a_frame::CLASS)?)?;
    if !flags.contains(&flag) {
        return None;
    }
    let payload_start = token_at.checked_add(usize::from(width))?;
    let end = payload_start.checked_add(length)?;
    if end > source_end {
        return None;
    }
    let header_token = data
        .get(token_at..payload_start)?
        .iter()
        .enumerate()
        .fold(0u32, |value, (shift, byte)| {
            value | (u32::from(*byte) << (8 * shift))
        });
    Some(ConsolidatedRecord {
        source_index: 0,
        source_range: pos..end,
        physically_contiguous: true,
        family,
        width,
        flag,
        class,
        header_token,
        range: pos..end,
        payload: payload_start..end,
    })
}

pub(crate) fn a_family_frames_from_records(
    records: &[ConsolidatedRecord],
    class: u8,
) -> Vec<ConsolidatedFrame> {
    records
        .iter()
        .filter(|record| {
            record.physically_contiguous
                && record.family == ConsolidatedFamily::A
                && record.class == class
        })
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

pub(crate) fn b_family_frames_from_records(
    records: &[ConsolidatedRecord],
    class: u8,
) -> Vec<ConsolidatedFrame> {
    records
        .iter()
        .filter(|record| {
            record.physically_contiguous
                && record.family == ConsolidatedFamily::B
                && record.class == class
        })
        .map(|record| ConsolidatedFrame {
            pos: record.range.start,
            payload: record.payload.start,
            end: record.range.end,
            header_token: record.header_token,
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn b_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    let records = consolidated_records(data);
    b_family_frames_from_records(&records, class)
}

/// Scan every `05 08 01` coordinate row in `bytes`, returning the decoded
/// vertex points in stream order.
pub fn scan_vertex_records(bytes: &[u8]) -> Vec<Point3> {
    scan_vertex_record_ranges(bytes)
        .into_iter()
        .map(|range| {
            let x = f32_le(bytes, range.start + 3);
            let y = f32_le(bytes, range.start + 7);
            let z = f32_le(bytes, range.start + 11);
            Point3::new(x as f64, y as f64, z as f64)
        })
        .collect()
}

/// Locate every finite `05 08 01` coordinate row in `bytes`.
pub(crate) fn scan_vertex_record_ranges(bytes: &[u8]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut p = 0usize;
    while p + 15 <= bytes.len() {
        if bytes[p] == 0x05 && bytes[p + 1] == 0x08 && bytes[p + 2] == 0x01 {
            let x = f32_le(bytes, p + 3);
            let y = f32_le(bytes, p + 7);
            let z = f32_le(bytes, p + 11);
            if x.is_finite() && y.is_finite() && z.is_finite() {
                ranges.push(p..p + 15);
            }
            p += 15;
        } else {
            p += 1;
        }
    }
    ranges
}

fn f32_le(bytes: &[u8], at: usize) -> f32 {
    let mut view = View::over_retained(bytes);
    view.seek(at)
        .and_then(|()| view.f32_le())
        .unwrap_or(f32::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_walk_does_not_rescan_a_wide_header_token() {
        let mut bytes = vec![0xa7, 0x03, 0x20];
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0xa5, 0x03, 0x20]);
        bytes.extend_from_slice(&[0; 8]);

        let records = consolidated_records(&bytes);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].family, ConsolidatedFamily::A);
        assert_eq!(records[0].range, 0..18);
    }

    #[test]
    fn bounded_record_walk_does_not_cross_an_extent_boundary() {
        let mut bytes = vec![0xa5, 0x03, 0x20];
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0x05, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0xb2, 0x03, 0x20, 0x01, 0x05, 0]);

        let records = consolidated_records_in_ranges(&bytes, [0..12, 12..bytes.len()]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].range, 15..21);
        assert_eq!(records[0].source_index, 1);
        assert_eq!(records[0].family, ConsolidatedFamily::B);
    }

    #[test]
    fn bounded_record_walk_ignores_unselected_file_regions() {
        let mut bytes = vec![0xa5, 0x03, 0x20];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0x05, 0]);
        bytes.extend_from_slice(&[0xa5, 0x03, 0x20]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0x05, 0]);

        let records = consolidated_records_in_ranges(&bytes, std::iter::once(9..bytes.len()));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].range, 9..18);
    }

    #[test]
    fn logical_walk_retains_a_frame_that_spans_physical_extents() {
        let mut bytes = vec![0xb2, 0x03, 0x20, 0x01, 0x05, 0];
        let spanning_start = bytes.len();
        bytes.extend_from_slice(&[0xa5, 0x03, 0x34]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0x05, 0, 1, 2, 3, 4, 5, 6, 7]);
        let split = spanning_start + 10;

        let records = consolidated_records_in_sources(&bytes, [[0..split, split..bytes.len()]]);

        assert_eq!(records.len(), 2);
        assert_eq!(records[1].family, ConsolidatedFamily::A);
        assert_eq!(records[1].class, 0x34);
        assert_eq!(records[1].source_range, spanning_start..bytes.len());
        assert!(!records[1].physically_contiguous);
        assert!(a_family_frames_from_records(&records, 0x34).is_empty());
    }

    #[test]
    fn record_runs_require_logical_source_adjacency() {
        let first = ConsolidatedRecord {
            source_index: 0,
            source_range: 0..4,
            physically_contiguous: true,
            family: ConsolidatedFamily::A,
            width: 1,
            flag: 0x03,
            class: 0x20,
            header_token: 0,
            range: 0..4,
            payload: 3..4,
        };
        let adjacent = ConsolidatedRecord {
            source_range: 4..8,
            range: 4..8,
            ..first.clone()
        };
        let separated = ConsolidatedRecord {
            source_range: 5..9,
            range: 5..9,
            ..first.clone()
        };
        let adjacent_other_source = ConsolidatedRecord {
            source_index: 1,
            source_range: 4..8,
            range: 4..8,
            ..first.clone()
        };

        assert!(super::records_are_contiguous(&[first.clone(), adjacent]));
        assert!(!super::records_are_contiguous(&[
            first.clone(),
            adjacent_other_source
        ]));
        assert!(!super::records_are_contiguous(&[first, separated]));
    }

    #[test]
    fn vertex_scanner_accepts_finite_coordinates_without_model_size_cutoff() {
        let mut bytes = vec![0x05, 0x08, 0x01];
        for value in [2_000_000.0_f32, -2_000_000.0, 2_000_000.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        let [point] = scan_vertex_records(&bytes)
            .try_into()
            .expect("one vertex row");
        assert_eq!(point.x, 2_000_000.0);
        assert_eq!(point.y, -2_000_000.0);
        assert_eq!(point.z, 2_000_000.0);
    }
}
