// SPDX-License-Identifier: Apache-2.0
//! Generic CATIA record-walking and wire-shape record decoding.
//!
//! The family-independent scan layer: length-closed A/B record framing
//! (`consolidated_records` and its `a_family_frames`/`b_family_frames` views),
//! the `05 08 01` vertex-coordinate scanner (`scan_vertex_records`), and the
//! degree-5 UV jet decoder (`parse_consolidated_pcurve`). Nothing here depends
//! on a `families` module; the family decoders consume it downward.

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::decode::u64_from_index;

type NativePcurveLanes = (
    Vec<FiniteReal>,
    Vec<FiniteVector<2>>,
    Vec<FiniteVector<2>>,
    Vec<FiniteVector<2>>,
);

use std::{borrow::Borrow, ops::Range};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::FiniteVector;
use serde::{Deserialize, Serialize};

use crate::layout::a_family_frame as a_frame;
use crate::layout::b_family_frame as b_frame;

use super::bytes::{compact_int, f64_le};

/// One knot of a degree-5 consolidated UV jet.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConsolidatedPcurveSite {
    /// Global parameter at this site.
    pub(crate) knot: FiniteReal,
    /// UV position.
    pub(crate) point: FiniteVector<2>,
    /// UV first derivative.
    pub(crate) first_derivatives: FiniteVector<2>,
    /// UV second derivative.
    pub(crate) second_derivatives: FiniteVector<2>,
}

/// At least two UV jet sites in strict knot order. Only immutable access is exposed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OrderedPcurveSites(Vec<ConsolidatedPcurveSite>);

impl TryFrom<Vec<ConsolidatedPcurveSite>> for OrderedPcurveSites {
    type Error = &'static str;
    fn try_from(sites: Vec<ConsolidatedPcurveSite>) -> Result<Self, Self::Error> {
        if sites.len() < 2
            || sites
                .windows(2)
                .any(|pair| pair[0].knot.get() >= pair[1].knot.get())
        {
            return Err("pcurve sites require at least two strictly increasing knots");
        }
        Ok(Self(sites))
    }
}

impl std::ops::Deref for OrderedPcurveSites {
    type Target = [ConsolidatedPcurveSite];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a OrderedPcurveSites {
    type Item = &'a ConsolidatedPcurveSite;
    type IntoIter = std::slice::Iter<'a, ConsolidatedPcurveSite>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Degree-5 UV jet stored in an A- or B-family class-`0x20` consolidated record.
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedPcurve {
    /// Record byte offset.
    pub(crate) pos: usize,
    /// Referenced support-surface identifier.
    pub(crate) support_id: u32,
    /// Number of leading extrapolation sites encoded by the array marker.
    pub(crate) extrapolation_sites: u32,
    /// Knot-aligned UV jet samples.
    pub(crate) sites: OrderedPcurveSites,
    /// Native parameter range.
    pub(crate) range: IncreasingParameterInterval,
    /// Bytes following the native range inside the framed record.
    pub(crate) tail: Vec<u8>,
}

/// Decode class-`0x20` UV jets from one framed record family.
pub(crate) fn family_pcurves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
    family: ConsolidatedFamily,
) -> Vec<ConsolidatedPcurve> {
    let mut pcurves = Vec::new();
    for frame in family_frames_from_records(records, family, 0x20) {
        if let Some(pcurve) =
            parse_consolidated_pcurve(data, frame.pos, frame.payload, frame.end, false)
        {
            pcurves.push(pcurve);
        }
    }
    pcurves
}

pub(crate) fn owned_pcurve_from_record(
    data: &[u8],
    record: &ConsolidatedRecord,
) -> Option<ConsolidatedPcurve> {
    let frame =
        family_frames_from_records(std::slice::from_ref(record), record.family(), 0x20).next()?;
    parse_consolidated_pcurve(data, frame.pos, frame.payload, frame.end, true)
}

impl ConsolidatedPcurve {
    pub(crate) const DEGREE: u32 = 5;

    pub(crate) fn native_lanes(&self) -> NativePcurveLanes {
        let mut knots = Vec::new();
        let mut points = Vec::new();
        let mut first = Vec::new();
        let mut second = Vec::new();

        for site in &self.sites {
            knots.push(site.knot);
            points.push(site.point);
            first.push(site.first_derivatives);
            second.push(site.second_derivatives);
        }
        (knots, points, first, second)
    }

    #[cfg(test)]
    pub(crate) fn knots(&self) -> Vec<FiniteReal> {
        self.sites.iter().map(|site| site.knot).collect()
    }

    #[cfg(test)]
    pub(crate) fn points(&self) -> Vec<FiniteVector<2>> {
        self.sites.iter().map(|site| site.point).collect()
    }

    #[cfg(test)]
    pub(crate) fn second_derivatives(&self) -> Vec<FiniteVector<2>> {
        self.sites
            .iter()
            .map(|site| site.second_derivatives)
            .collect()
    }
}

fn parse_consolidated_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
    owned: bool,
) -> Option<ConsolidatedPcurve> {
    let (support_id, count, extrapolation_sites, lanes, range, tail_at) =
        pcurve_layout(data, payload, end, owned)?;

    let mut sites = Vec::new();

    for index in 0..count {
        let offset = index * 8;
        let values = lanes.map(|lane| f64_le(data, lane + offset));
        let [Some(knot), Some(u), Some(v), Some(du), Some(dv), Some(ddu), Some(ddv)] = values
        else {
            return None;
        };
        sites.push(ConsolidatedPcurveSite {
            knot,
            point: [u, v].into(),
            first_derivatives: [du, dv].into(),
            second_derivatives: [ddu, ddv].into(),
        });
    }

    let Ok(sites) = OrderedPcurveSites::try_from(sites) else {
        return None;
    };
    Some(ConsolidatedPcurve {
        pos,
        support_id,
        extrapolation_sites,
        sites,
        range,
        tail: data[tail_at..end].to_vec(),
    })
}

fn pcurve_layout(
    data: &[u8],
    payload: usize,
    end: usize,
    owned: bool,
) -> Option<(
    u32,
    usize,
    u32,
    [usize; 7],
    IncreasingParameterInterval,
    usize,
)> {
    use crate::layout::owned_pcurve_wide_channel_header as wide_channel;

    let mut at = payload;
    let support_id = compact_int(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    if degree != 5 || count < 2 {
        return None;
    }
    let mut wide_extrapolation = None;
    let extrapolation_sites = match *data.get(at)? {
        0x0c => {
            at += 1;
            0
        }
        0x08 => {
            let encoded = *data.get(at + 1)?;
            match encoded {
                0x06 if owned => {
                    let count = data.get(at + 2).copied().filter(|&count| count >= 0x40)?;
                    wide_extrapolation = Some(count);
                    at += 3;
                    u32::from(count)
                }
                code if matches!(code % 4, 1 | 3) => {
                    at += 2;
                    u32::from((code - 1) / 4)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let knot_bytes = count.checked_mul(8)?;
    if at.checked_add(knot_bytes.checked_add(20)?)? > end {
        return None;
    }
    let knot_at = at;
    at += knot_bytes;
    if owned && wide_extrapolation.is_none() && data.get(at) == Some(&wide_channel::MARKER_VALUE) {
        if usize::from(View::u16_le_at(
            data.get(at..end)?,
            wide_channel::SITE_COUNT,
        )?) != count
        {
            return None;
        }
        at = at.checked_add(wide_channel::LEN)?;
    } else {
        if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
            return None;
        }
        at = if let Some(count) = wide_extrapolation {
            if data.get(at..at.checked_add(2)?)? != [0x06, count] {
                return None;
            }
            at.checked_add(2)?
        } else {
            at.checked_add(1)?
        };
    }
    if at > end {
        return None;
    }
    let remaining_array_bytes = count.checked_mul(8)?.checked_mul(6)?;
    if at.checked_add(remaining_array_bytes.checked_add(18)?)? > end {
        return None;
    }
    let u_at = at;
    at += knot_bytes;
    let v_at = at;
    at += knot_bytes;
    let du_at = at;
    at += knot_bytes;
    let dv_at = at;
    at += knot_bytes;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu_at = at;
    at += knot_bytes;
    let ddv_at = at;
    at += knot_bytes;
    let range =
        IncreasingParameterInterval::new([f64_le(data, at)?.get(), f64_le(data, at + 8)?.get()])?;
    at += 16;
    let lanes = [knot_at, u_at, v_at, du_at, dv_at, ddu_at, ddv_at];
    match data.get(at..end)? {
        [0x07] | [0x07, 0x00] => {}
        [0x07, 0x01, terminal, 0x00] if owned && terminal % 2 == 1 => {}
        _ => return None,
    }
    Some((support_id, count, extrapolation_sites, lanes, range, at))
}

/// Header-token width of a length-closed A/B-family frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum ConsolidatedFrameWidth {
    /// One-byte header token.
    One,
    /// Two-byte header token.
    Two,
    /// Three-byte header token.
    Three,
    /// Four-byte header token in an allocation run.
    Four,
}

impl From<ConsolidatedFrameWidth> for u8 {
    fn from(value: ConsolidatedFrameWidth) -> Self {
        match value {
            ConsolidatedFrameWidth::One => 1,
            ConsolidatedFrameWidth::Two => 2,
            ConsolidatedFrameWidth::Three => 3,
            ConsolidatedFrameWidth::Four => 4,
        }
    }
}

impl ConsolidatedFrameWidth {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::One),
            0x02 => Some(Self::Two),
            0x03 => Some(Self::Three),
            _ => None,
        }
    }
}

impl TryFrom<u8> for ConsolidatedFrameWidth {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value == 4 {
            Ok(Self::Four)
        } else {
            Self::from_byte(value).ok_or_else(|| format!("width {value} is not 1..=4"))
        }
    }
}

/// Independent framing flag of a length-closed A/B-family frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum ConsolidatedFrameFlag {
    /// Flag `0x03`.
    Flag03,
    /// Flag `0x13`.
    Flag13,
    /// Flag `0x83`.
    Flag83,
}

impl From<ConsolidatedFrameFlag> for u8 {
    fn from(value: ConsolidatedFrameFlag) -> Self {
        match value {
            ConsolidatedFrameFlag::Flag03 => 0x03,
            ConsolidatedFrameFlag::Flag13 => 0x13,
            ConsolidatedFrameFlag::Flag83 => 0x83,
        }
    }
}

impl ConsolidatedFrameFlag {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x03 => Some(Self::Flag03),
            0x13 => Some(Self::Flag13),
            0x83 => Some(Self::Flag83),
            _ => None,
        }
    }
}

impl TryFrom<u8> for ConsolidatedFrameFlag {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_byte(value).ok_or_else(|| format!("flag {value:#x} is not 0x03, 0x13, or 0x83"))
    }
}

/// A token that fits its declared one-, two-, or three-byte width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "WidthCodedTokenWire", into = "WidthCodedTokenWire")]
pub(crate) struct WidthCodedToken {
    width: ConsolidatedFrameWidth,
    value: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WidthCodedTokenWire {
    width: ConsolidatedFrameWidth,
    header_token: u32,
}

impl TryFrom<WidthCodedTokenWire> for WidthCodedToken {
    type Error = &'static str;
    fn try_from(wire: WidthCodedTokenWire) -> Result<Self, Self::Error> {
        Self::new(wire.width, wire.header_token)
    }
}

impl From<WidthCodedToken> for WidthCodedTokenWire {
    fn from(value: WidthCodedToken) -> Self {
        Self {
            width: value.width,
            header_token: value.value,
        }
    }
}

impl WidthCodedToken {
    pub(crate) fn new(width: ConsolidatedFrameWidth, value: u32) -> Result<Self, &'static str> {
        let maximum = match width {
            ConsolidatedFrameWidth::One => 0xff,
            ConsolidatedFrameWidth::Two => 0xffff,
            ConsolidatedFrameWidth::Three => 0xff_ffff,
            ConsolidatedFrameWidth::Four => u32::MAX,
        };
        if value > maximum {
            return Err("header token does not fit its declared width");
        }
        Ok(Self { width, value })
    }
    pub(crate) fn compact_operand(self) -> Option<crate::wire::compact_unsigned::CompactUnsigned> {
        let bytes = self.value.to_le_bytes();
        let bytes = bytes.get(..usize::from(u8::from(self.width)))?;
        let mut at = 0;
        let operand = crate::wire::compact_unsigned::CompactUnsigned::read(bytes, &mut at)?;
        (at == bytes.len()).then_some(operand)
    }
    pub(crate) fn width(self) -> ConsolidatedFrameWidth {
        self.width
    }
    pub(crate) fn value(self) -> u32 {
        self.value
    }
}

/// Length-closed A/B-family frame shared by edge-definition and descriptor records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsolidatedRawFrame<Offset = usize> {
    /// Record byte offset.
    pub(crate) pos: Offset,
    token: WidthCodedToken,
    /// Independent framing flag.
    pub(crate) flag: ConsolidatedFrameFlag,
    /// Complete class-specific payload.
    pub(crate) payload: cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>>,
}

#[derive(Deserialize)]
struct RawFrameWire<Offset> {
    byte_offset: Offset,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    header_token: u32,
    payload: NativeBytes<Vec<u8>>,
}

impl<Offset> ConsolidatedRawFrame<Offset> {
    pub(crate) fn new(
        pos: Offset,
        width: ConsolidatedFrameWidth,
        flag: ConsolidatedFrameFlag,
        header_token: u32,
        payload: Vec<u8>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            pos,
            token: WidthCodedToken::new(width, header_token)?,
            flag,
            payload: (payload).into(),
        })
    }
    pub(crate) fn width(&self) -> ConsolidatedFrameWidth {
        self.token.width()
    }
    pub(crate) fn header_token(&self) -> u32 {
        self.token.value()
    }
}

impl<Offset: Serialize> Serialize for ConsolidatedRawFrame<Offset> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("ConsolidatedRawFrame", 5)?;
        wire.serialize_field("byte_offset", &self.pos)?;
        wire.serialize_field("width", &self.width())?;
        wire.serialize_field("flag", &self.flag)?;
        wire.serialize_field("header_token", &self.header_token())?;
        wire.serialize_field("payload", &self.payload)?;
        wire.end()
    }
}

impl<'de, Offset: Deserialize<'de>> Deserialize<'de> for ConsolidatedRawFrame<Offset> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RawFrameWire::<Offset>::deserialize(deserializer)?;
        Self::new(
            wire.byte_offset,
            wire.width,
            wire.flag,
            wire.header_token,
            wire.payload.into_inner(),
        )
        .map_err(serde::de::Error::custom)
    }
}

impl ConsolidatedRawFrame {
    pub(crate) fn from_record(
        record: &ConsolidatedRecord,
        payload: Vec<u8>,
    ) -> Result<Self, CodecError> {
        Self::new(
            record.byte_offset(),
            record.width(),
            record.flag,
            record.header_token(),
            payload,
        )
        .map_err(CodecError::malformed)
    }
}

impl From<ConsolidatedRawFrame> for ConsolidatedRawFrame<u64> {
    fn from(frame: ConsolidatedRawFrame) -> Self {
        Self {
            pos: u64_from_index(frame.pos),
            token: frame.token,
            flag: frame.flag,
            payload: (frame.payload),
        }
    }
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
pub(crate) enum ConsolidatedFamily {
    /// U32-length A family (`a5/a6/a7`).
    A,
    /// U8-length B family (`b2/b3/b4`).
    B,
}

/// One length-closed record in a consolidated A/B cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsolidatedRecord {
    /// Zero-based logical record-source ordinal supplied by the container.
    source_index: usize,
    /// Byte range in the reconstructed logical record source.
    source_range: Range<usize>,
    /// Physical placement of the frame.
    placement: ConsolidatedPlacement,
    /// Record family.
    family: ConsolidatedFamily,
    /// Header token admitted together with its byte width.
    token: WidthCodedToken,
    /// Independent flag byte (`0x03`, `0x13`, or `0x83`).
    flag: ConsolidatedFrameFlag,
    /// Record class byte.
    class: u8,
}

/// Physical placement of a consolidated frame.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsolidatedPlacement {
    /// A frame contained in one physical extent.
    Contiguous {
        /// Complete physical frame range.
        range: Range<usize>,
        /// Physical payload range.
        payload: Range<usize>,
    },
    /// A frame crossing physical extents.
    Spanning {
        /// Physical start offset.
        byte_offset: usize,
    },
}

impl ConsolidatedRecord {
    pub(crate) fn width(&self) -> ConsolidatedFrameWidth {
        self.token.width()
    }
    pub(crate) fn header_token(&self) -> u32 {
        self.token.value()
    }
    pub(crate) fn source_index(&self) -> usize {
        self.source_index
    }
    pub(crate) fn payload_bytes<'a>(
        &self,
        data: &'a [u8],
        sources: &[Vec<SourceExtent>],
    ) -> Option<std::borrow::Cow<'a, [u8]>> {
        match &self.placement {
            ConsolidatedPlacement::Contiguous { payload, .. } => {
                Some(std::borrow::Cow::Borrowed(data.get(payload.clone())?))
            }
            ConsolidatedPlacement::Spanning { .. } => {
                let header_len = match self.family {
                    ConsolidatedFamily::A => a_frame::LEN,
                    ConsolidatedFamily::B => b_frame::LEN,
                };
                let prefix = header_len.checked_add(usize::from(u8::from(self.width())))?;
                let payload_start = self.source_range.start.checked_add(prefix)?;
                let payload_end = self.source_range.end;
                if payload_start > payload_end {
                    return None;
                }
                let extents = sources.get(self.source_index)?;
                let mut out = Vec::with_capacity(payload_end - payload_start);
                let mut logical = 0usize;
                for extent in extents {
                    let range = extent.range();
                    let ext_len = range.end.checked_sub(range.start)?;
                    let ext_end = logical.checked_add(ext_len)?;
                    if ext_end > payload_start && logical < payload_end {
                        let copy_start = payload_start.max(logical) - logical;
                        let copy_end = payload_end.min(ext_end) - logical;
                        out.extend_from_slice(
                            data.get(range.start + copy_start..range.start + copy_end)?,
                        );
                    }
                    logical = ext_end;
                    if logical >= payload_end {
                        break;
                    }
                }
                (out.len() == payload_end - payload_start).then_some(std::borrow::Cow::Owned(out))
            }
        }
    }
    pub(crate) fn source_range(&self) -> &Range<usize> {
        &self.source_range
    }
    pub(crate) fn family(&self) -> ConsolidatedFamily {
        self.family
    }
    pub(crate) fn flag(&self) -> ConsolidatedFrameFlag {
        self.flag
    }
    pub(crate) fn class(&self) -> u8 {
        self.class
    }

    pub(crate) fn byte_offset(&self) -> usize {
        match &self.placement {
            ConsolidatedPlacement::Contiguous { range, .. } => range.start,
            ConsolidatedPlacement::Spanning { byte_offset } => *byte_offset,
        }
    }

    pub(crate) fn range(&self) -> Option<Range<usize>> {
        match &self.placement {
            ConsolidatedPlacement::Contiguous { range, .. } => Some(range.clone()),
            ConsolidatedPlacement::Spanning { .. } => None,
        }
    }

    pub(crate) fn payload(&self) -> Option<Range<usize>> {
        match &self.placement {
            ConsolidatedPlacement::Contiguous { payload, .. } => Some(payload.clone()),
            ConsolidatedPlacement::Spanning { .. } => None,
        }
    }
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
#[cfg(test)]
pub(crate) fn consolidated_records(data: &[u8]) -> Vec<ConsolidatedRecord> {
    consolidated_records_in_sources(
        data,
        std::iter::once(std::iter::once(SourceExtent::whole(data))),
    )
    .expect("read the consolidated records from the sources")
}

/// A physical record-source extent proved to lie inside the image it indexes.
///
/// A source extent comes from a parsed stream descriptor. The record scanner
/// reads every byte of the extent, so an extent the image does not hold cannot
/// be shortened to fit it: [`SourceExtent::within`] refuses to build one, and
/// the type has no other constructor from parsed values. The complete image is
/// always its own extent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SourceExtent(Range<usize>);

impl SourceExtent {
    /// The complete image.
    #[must_use]
    pub(crate) fn whole(data: &[u8]) -> Self {
        Self(0..data.len())
    }

    /// One declared extent of `data`.
    ///
    /// A declared end past the end of the image, or a start past its own end,
    /// is not an extent of this image and states no record source.
    #[must_use]
    pub(crate) fn within(data: &[u8], start: usize, end: usize) -> Option<Self> {
        (start <= end && end <= data.len()).then_some(Self(start..end))
    }

    /// The proved byte range within the image.
    #[must_use]
    pub(crate) fn range(&self) -> Range<usize> {
        self.0.clone()
    }
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
    consolidated_records_in_sources(
        data,
        ranges
            .into_iter()
            .filter_map(|range| SourceExtent::within(data, range.start, range.end))
            .map(std::iter::once),
    )
    .expect("read the consolidated records from the sources")
}

/// Inventory records in descriptor-scoped logical sources stated as byte
/// ranges of `data`.
///
/// A range the image does not hold is not an extent of it and states no record
/// source. Fixture builders state ranges directly; container decode paths carry
/// [`SourceExtent`] values proved at the directory.
#[cfg(test)]
pub(crate) fn consolidated_records_in_range_sources<S, R>(
    data: &[u8],
    sources: S,
) -> Vec<ConsolidatedRecord>
where
    S: IntoIterator<Item = R>,
    R: IntoIterator<Item = Range<usize>>,
{
    consolidated_records_in_sources(
        data,
        sources.into_iter().map(|ranges| {
            ranges
                .into_iter()
                .filter_map(|range| SourceExtent::within(data, range.start, range.end))
                .collect::<Vec<_>>()
        }),
    )
    .expect("read the consolidated records from the sources")
}

/// Inventory records in descriptor-scoped logical sources. Physical extents
/// within one source are visited in logical-offset order. A frame that starts
/// immediately after a retained frame and spans an extent boundary is retained
/// as an ordinal-bearing non-contiguous frame; typed payload decoding requires
/// one physical extent.
pub(crate) fn consolidated_records_in_sources<S, R>(
    data: &[u8],
    sources: S,
) -> Result<Vec<ConsolidatedRecord>, CodecError>
where
    S: IntoIterator<Item = R>,
    R: IntoIterator,
    R::Item: Borrow<SourceExtent>,
{
    let mut records = Vec::new();
    for (source_index, extents) in sources.into_iter().enumerate() {
        // Revalidate extents against this image before using their ranges.
        let mut source_ranges = Vec::new();
        for extent in extents {
            let range = extent.borrow().range();
            if range.end > data.len() || range.start > range.end {
                return Err(CodecError::malformed(
                    "record source extent is outside the supplied image",
                ));
            }
            if range.start < range.end {
                source_ranges.push(range);
            }
        }
        let mut source_records: Vec<ConsolidatedRecord> = Vec::new();
        let mut extent_logical_starts = Vec::with_capacity(source_ranges.len());
        let mut source_length = 0usize;
        for range in &source_ranges {
            extent_logical_starts.push(source_length);
            let Some(next_length) = source_length.checked_add(range.end - range.start) else {
                return Ok(records);
            };
            source_length = next_length;
        }
        let mut extent_idx = 0usize;
        let mut logical_pos = 0usize;
        while logical_pos < source_length {
            while extent_idx < source_ranges.len() {
                let ext_start = extent_logical_starts[extent_idx];
                let ext_len = source_ranges[extent_idx].end - source_ranges[extent_idx].start;
                if logical_pos < ext_start + ext_len {
                    break;
                }
                extent_idx += 1;
            }
            let Some(range) = source_ranges.get(extent_idx) else {
                break;
            };
            let ext_logical_start = extent_logical_starts[extent_idx];
            let phys_pos = range.start + (logical_pos - ext_logical_start);
            let continuing_run = source_records
                .last()
                .is_some_and(|prev| prev.source_range.end == logical_pos);

            if let Some(mut record) =
                parse_consolidated_record(data, phys_pos, range.end).or_else(|| {
                    continuing_run.then(|| {
                        parse_consolidated_allocation_record(data, phys_pos, range.end).filter(
                            |rec| {
                                rec.flag() == ConsolidatedFrameFlag::Flag03
                                    && WidthCodedToken::new(rec.width(), rec.header_token())
                                        .ok()
                                        .and_then(WidthCodedToken::compact_operand)
                                        .is_some()
                            },
                        )
                    })?
                })
            {
                let Some(physical_range) = record.range() else {
                    return Ok(records);
                };
                let Some(source_start) =
                    ext_logical_start.checked_add(physical_range.start - range.start)
                else {
                    return Ok(records);
                };
                let Some(source_end) =
                    ext_logical_start.checked_add(physical_range.end - range.start)
                else {
                    return Ok(records);
                };
                record.source_index = source_index;
                record.source_range = source_start..source_end;
                logical_pos = source_end;
                source_records.push(record);
                continue;
            }

            if continuing_run {
                if let Some(record) = parse_spanning_consolidated_record(
                    data,
                    &source_ranges,
                    source_index,
                    logical_pos,
                ) {
                    logical_pos = record.source_range.end;
                    source_records.push(record);
                    continue;
                }
            }

            logical_pos += 1;
        }

        records.extend(source_records);
    }
    Ok(records)
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
    let (family, width, header_len, length) = if let Some(width) =
        first.checked_sub(0xa4).and_then(|raw| {
            if raw == 4 {
                Some(ConsolidatedFrameWidth::Four)
            } else {
                ConsolidatedFrameWidth::from_byte(raw)
            }
        }) {
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
        let width = match first {
            0xb5 => ConsolidatedFrameWidth::Four,
            marker => ConsolidatedFrameWidth::from_byte(marker.checked_sub(0xb1)?)?,
        };
        (
            ConsolidatedFamily::B,
            width,
            b_frame::LEN,
            usize::from(source_byte(
                source_start.checked_add(b_frame::PAYLOAD_LEN)?,
            )?),
        )
    };
    let flag =
        ConsolidatedFrameFlag::from_byte(source_byte(source_start.checked_add(a_frame::FLAG)?)?)?;
    let class = source_byte(source_start.checked_add(a_frame::CLASS)?)?;
    let token_at = source_start.checked_add(header_len)?;
    let payload_start = token_at.checked_add(usize::from(u8::from(width)))?;
    let source_end = payload_start.checked_add(length)?;
    if source_end > source_length {
        return None;
    }
    let mut logical_boundary = 0usize;
    let mut crosses_extent = false;
    let (_, preceding_ranges) = ranges.split_last()?;
    for range in preceding_ranges {
        logical_boundary = logical_boundary.checked_add(range.end - range.start)?;
        crosses_extent |= source_start < logical_boundary && logical_boundary < source_end;
    }
    if !crosses_extent {
        return None;
    }
    let header_token = (0..usize::from(u8::from(width))).try_fold(0u32, |value, relative| {
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
    let token = WidthCodedToken::new(width, header_token).ok()?;
    if width == ConsolidatedFrameWidth::Four
        && (flag != ConsolidatedFrameFlag::Flag03 || token.compact_operand().is_none())
    {
        return None;
    }
    Some(ConsolidatedRecord {
        source_index,
        source_range: source_start..source_end,
        placement: ConsolidatedPlacement::Spanning { byte_offset },
        family,
        token,
        flag,
        class,
    })
}

pub(crate) fn parse_consolidated_record(
    data: &[u8],
    pos: usize,
    source_end: usize,
) -> Option<ConsolidatedRecord> {
    parse_record(data, pos, source_end, false)
}

pub(crate) fn parse_consolidated_allocation_record(
    data: &[u8],
    pos: usize,
    source_end: usize,
) -> Option<ConsolidatedRecord> {
    parse_record(data, pos, source_end, true)
}

fn parse_record(
    data: &[u8],
    pos: usize,
    source_end: usize,
    allocation_run: bool,
) -> Option<ConsolidatedRecord> {
    let (family, width, token_at, length) = if let Some(width) = data
        .get(pos)
        .and_then(|byte| byte.checked_sub(0xa4))
        .and_then(|width| {
            if allocation_run && width == 4 {
                Some(ConsolidatedFrameWidth::Four)
            } else {
                ConsolidatedFrameWidth::from_byte(width)
            }
        }) {
        let length = View::u32_le_at(data, pos.checked_add(a_frame::PAYLOAD_LEN)?)
            .and_then(|value| usize::try_from(value).ok())?;
        (
            ConsolidatedFamily::A,
            width,
            pos.checked_add(a_frame::LEN)?,
            length,
        )
    } else {
        let width = match *data.get(pos)? {
            0xb5 if allocation_run => ConsolidatedFrameWidth::Four,
            marker => ConsolidatedFrameWidth::from_byte(marker.checked_sub(0xb1)?)?,
        };
        (
            ConsolidatedFamily::B,
            width,
            pos.checked_add(b_frame::LEN)?,
            usize::from(*data.get(pos.checked_add(b_frame::PAYLOAD_LEN)?)?),
        )
    };
    let flag = ConsolidatedFrameFlag::from_byte(*data.get(pos.checked_add(a_frame::FLAG)?)?)?;
    let class = *data.get(pos.checked_add(a_frame::CLASS)?)?;
    let payload_start = token_at.checked_add(usize::from(u8::from(width)))?;
    let end = payload_start.checked_add(length)?;
    if end > source_end || end > data.len() {
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
        family,
        token: WidthCodedToken::new(width, header_token).ok()?,
        flag,
        class,
        placement: ConsolidatedPlacement::Contiguous {
            range: pos..end,
            payload: payload_start..end,
        },
    })
}

pub(crate) fn family_frames_from_records(
    records: &[ConsolidatedRecord],
    family: ConsolidatedFamily,
    class: u8,
) -> impl Iterator<Item = ConsolidatedFrame> + '_ {
    records
        .iter()
        .filter(move |record| record.family == family && record.class == class)
        .filter_map(|record| {
            Some(ConsolidatedFrame {
                pos: record.byte_offset(),
                payload: record.payload()?.start,
                end: record.range()?.end,
                header_token: record.header_token(),
            })
        })
}

#[cfg(test)]
pub(crate) fn b_family_frames(data: &[u8], class: u8) -> Vec<ConsolidatedFrame> {
    let records = consolidated_records(data);
    family_frames_from_records(&records, ConsolidatedFamily::B, class).collect()
}

/// Scan every `05 08 01` coordinate row in `bytes`, returning the decoded
/// vertex points in stream order.
pub(crate) fn scan_vertex_records(bytes: &[u8]) -> impl Iterator<Item = FinitePoint3> + '_ {
    scan_vertex_rows(bytes).map(|(_, point)| point)
}

/// Locate every finite `05 08 01` coordinate row in `bytes`.
pub(crate) fn scan_vertex_record_ranges(bytes: &[u8]) -> impl Iterator<Item = Range<usize>> + '_ {
    scan_vertex_rows(bytes).map(|(range, _)| range)
}

/// Every `05 08 01` row whose three coordinates are finite, with its byte
/// range and admitted point.
fn scan_vertex_rows(bytes: &[u8]) -> impl Iterator<Item = (Range<usize>, FinitePoint3)> + '_ {
    let mut p = 0usize;
    std::iter::from_fn(move || loop {
        if p + 15 > bytes.len() {
            return None;
        }
        if bytes[p] == 0x05 && bytes[p + 1] == 0x08 && bytes[p + 2] == 0x01 {
            let x = f32_le(bytes, p + 3);
            let y = f32_le(bytes, p + 7);
            let z = f32_le(bytes, p + 11);
            let start = p;
            p += 15;
            if let Some(point) =
                FinitePoint3::new(Point3::new(f64::from(x), f64::from(y), f64::from(z)))
            {
                return Some((start..p, point));
            }
        } else {
            p += 1;
        }
    })
}

fn f32_le(bytes: &[u8], at: usize) -> f32 {
    let mut view = View::over_retained(bytes);
    view.seek(at)
        .and_then(|()| view.f32_le())
        .unwrap_or(f32::NAN)
}

#[cfg(test)]
mod tests {
    use super::{
        consolidated_records, consolidated_records_in_range_sources,
        consolidated_records_in_ranges, family_frames_from_records, scan_vertex_records,
        ConsolidatedFamily, ConsolidatedFrameFlag, ConsolidatedFrameWidth, ConsolidatedPlacement,
        ConsolidatedRecord,
    };
    use cadmpeg_core::serde_erase::json;

    #[test]
    fn raw_frame_token_fit_is_checked_in_construction_and_serde() {
        use super::ConsolidatedRawFrame;
        for (width, maximum) in [
            (ConsolidatedFrameWidth::One, 0xff),
            (ConsolidatedFrameWidth::Two, 0xffff),
            (ConsolidatedFrameWidth::Three, 0xff_ffff),
        ] {
            let frame = ConsolidatedRawFrame::new(
                12_u64,
                width,
                ConsolidatedFrameFlag::Flag03,
                maximum,
                vec![1],
            )
            .expect("fitting token");
            let bytes = json::to_vec(&frame).expect("frame JSON");
            assert_eq!(
                json::from_slice::<ConsolidatedRawFrame<u64>>(&bytes).expect("checked frame JSON"),
                frame
            );
            assert!(ConsolidatedRawFrame::new(
                12_u64,
                width,
                ConsolidatedFrameFlag::Flag03,
                maximum + 1,
                vec![1]
            )
            .is_err());
            let mut wire = json::to_value(&frame).expect("frame wire");
            wire["header_token"] = cadmpeg_core::json!(maximum + 1);
            assert!(json::from_value::<ConsolidatedRawFrame<u64>>(wire).is_err());
        }
        let frame = ConsolidatedRawFrame::new(
            12_u64,
            ConsolidatedFrameWidth::One,
            ConsolidatedFrameFlag::Flag03,
            5,
            vec![1],
        )
        .expect("frame");
        assert_eq!(
            json::to_string(&frame).expect("frame bytes"),
            r#"{"byte_offset":12,"width":1,"flag":3,"header_token":5,"payload":"01"}"#
        );
    }

    #[test]
    fn ordered_pcurve_sites_reject_empty_singleton_and_nonincreasing_knots() {
        let site = |knot| super::ConsolidatedPcurveSite {
            knot: cadmpeg_ir::scalar::FiniteReal::new(knot).expect("finite knot"),
            point: [cadmpeg_ir::scalar::FiniteReal::ZERO; 2].into(),
            first_derivatives: [cadmpeg_ir::scalar::FiniteReal::ZERO; 2].into(),
            second_derivatives: [cadmpeg_ir::scalar::FiniteReal::ZERO; 2].into(),
        };
        for sites in [
            vec![],
            vec![site(0.0)],
            vec![site(0.0), site(0.0)],
            vec![site(1.0), site(0.0)],
        ] {
            assert!(super::OrderedPcurveSites::try_from(sites).is_err());
        }
        let sites =
            super::OrderedPcurveSites::try_from(vec![site(0.0), site(1.0)]).expect("ordered knots");
        assert_eq!(
            sites.iter().map(|site| site.knot.get()).collect::<Vec<_>>(),
            [0.0, 1.0]
        );
    }

    #[test]
    fn borrowed_source_extents_build_record_inventory() {
        let bytes = [0xb2, 0x03, 0x06, 0x00, 0x05];
        let sources = [vec![super::SourceExtent::whole(&bytes)]];
        let records = super::consolidated_records_in_sources(
            &bytes,
            sources.iter().map(|source| source.iter()),
        )
        .expect("compute the consolidated records in sources");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].class, 0x06);
    }

    #[test]
    fn frame_states_admit_only_defined_widths_and_flags() {
        for byte in u8::MIN..=u8::MAX {
            let width = json::from_value::<ConsolidatedFrameWidth>(cadmpeg_core::json!(byte));
            assert_eq!(width.is_ok(), matches!(byte, 1..=4));
            if let Ok(width) = width {
                assert_eq!(
                    json::to_value(width).expect("serialize frame width"),
                    cadmpeg_core::json!(byte)
                );
            }
            let flag = json::from_value::<ConsolidatedFrameFlag>(cadmpeg_core::json!(byte));
            assert_eq!(flag.is_ok(), matches!(byte, 0x03 | 0x13 | 0x83));
            if let Ok(flag) = flag {
                assert_eq!(
                    json::to_value(flag).expect("serialize frame flag"),
                    cadmpeg_core::json!(byte)
                );
            }
        }
    }

    #[test]
    fn record_walk_does_not_rescan_a_wide_header_token() {
        let mut bytes = vec![0xa7, 0x03, 0x20];
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0xa5, 0x03, 0x20]);
        bytes.extend_from_slice(&[0; 8]);

        let records = consolidated_records(&bytes);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].family, ConsolidatedFamily::A);
        assert_eq!(records[0].range(), Some(0..18));
    }

    #[test]
    fn bounded_record_walk_does_not_cross_an_extent_boundary() {
        let mut bytes = vec![0xa5, 0x03, 0x20];
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0x05, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend_from_slice(&[0xb2, 0x03, 0x20, 0x01, 0x05, 0]);

        let records = consolidated_records_in_ranges(&bytes, [0..12, 12..bytes.len()]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].range(), Some(15..21));
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
        assert_eq!(records[0].range(), Some(9..18));
    }

    #[test]
    fn logical_walk_retains_a_frame_that_spans_physical_extents() {
        let mut bytes = vec![0xb2, 0x03, 0x20, 0x01, 0x05, 0];
        let spanning_start = bytes.len();
        bytes.extend_from_slice(&[0xa5, 0x03, 0x34]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&[0x05, 0, 1, 2, 3, 4, 5, 6, 7]);
        let split = spanning_start + 10;

        let records =
            consolidated_records_in_range_sources(&bytes, [[0..split, split..bytes.len()]]);

        assert_eq!(records.len(), 2);
        assert_eq!(records[1].family, ConsolidatedFamily::A);
        assert_eq!(records[1].class, 0x34);
        assert_eq!(records[1].source_range, spanning_start..bytes.len());
        assert!(records[1].range().is_none());
        assert!(
            family_frames_from_records(&records, ConsolidatedFamily::A, 0x34)
                .next()
                .is_none()
        );
    }

    #[test]
    fn record_runs_require_logical_source_adjacency() {
        let first = ConsolidatedRecord {
            source_index: 0,
            source_range: 0..4,
            family: ConsolidatedFamily::A,
            token: super::WidthCodedToken::new(ConsolidatedFrameWidth::One, 0)
                .expect("width-coded token"),
            flag: ConsolidatedFrameFlag::Flag03,
            class: 0x20,
            placement: ConsolidatedPlacement::Contiguous {
                range: 0..4,
                payload: 3..4,
            },
        };
        let adjacent = ConsolidatedRecord {
            source_range: 4..8,
            placement: ConsolidatedPlacement::Contiguous {
                range: 4..8,
                payload: 7..8,
            },
            ..first.clone()
        };
        let separated = ConsolidatedRecord {
            source_range: 5..9,
            placement: ConsolidatedPlacement::Contiguous {
                range: 5..9,
                payload: 7..8,
            },
            ..first.clone()
        };
        let adjacent_other_source = ConsolidatedRecord {
            source_index: 1,
            source_range: 4..8,
            placement: ConsolidatedPlacement::Contiguous {
                range: 4..8,
                payload: 7..8,
            },
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

        let [point] = { scan_vertex_records(&bytes).collect::<Vec<_>>() }
            .try_into()
            .expect("one vertex row");
        assert_eq!(point.x, 2_000_000.0);
        assert_eq!(point.y, -2_000_000.0);
        assert_eq!(point.z, 2_000_000.0);
    }

    #[test]
    fn single_frame_source_inventories_one_record_of_its_class() {
        let bytes = [0xb2, 0x03, 0x06, 0x00, 0x05];
        let records = super::consolidated_records_in_sources(
            &bytes,
            std::iter::once(std::iter::once(super::SourceExtent::whole(&bytes))),
        )
        .expect("read the consolidated records from the sources");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].class, 0x06);
    }

    #[test]
    fn pcurve_with_a_nan_in_its_last_site_is_dropped() {
        let mut bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        let records = consolidated_records(&bytes);
        let record = &records[0];
        let payload = record.payload().expect("payload");
        let (_, count, _, lanes, _, _) =
            super::pcurve_layout(&bytes, payload.start, payload.end, false).expect("layout");
        let late = lanes[6] + (count - 1) * 8;
        bytes[late..late + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(
            super::family_pcurves_from_records(&bytes, &records, ConsolidatedFamily::A).is_empty()
        );
    }

    #[test]
    fn a5_pcurve_reads_two_sites_and_a_one_byte_tail() {
        let bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        let records = consolidated_records(&bytes);
        let pcurves = super::family_pcurves_from_records(&bytes, &records, ConsolidatedFamily::A);
        assert_eq!(pcurves.len(), 1);
        assert_eq!(pcurves[0].sites.len(), 2);
        assert_eq!(pcurves[0].tail, [0x07]);
    }

    #[test]
    fn a5_pcurve_accepts_owned_array_marker_with_same_extrapolation_count() {
        let mut bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        bytes[14] = 0x0b;
        let records = consolidated_records(&bytes);
        let pcurves = super::family_pcurves_from_records(&bytes, &records, ConsolidatedFamily::A);
        assert_eq!(pcurves.len(), 1);
        assert_eq!(pcurves[0].extrapolation_sites, 2);
    }

    #[test]
    fn owned_pcurve_wide_extrapolation_requires_a_matching_wide_count() {
        for (leading, repeated, expected) in [(64, 64, Some(64)), (64, 65, None), (63, 63, None)] {
            let mut bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
            bytes.splice(32..33, [0x06, repeated]);
            bytes.splice(14..15, [0x06, leading]);
            let length = u32::try_from(bytes.len() - 8).expect("small frame");
            bytes[3..7].copy_from_slice(&length.to_le_bytes());

            let records = consolidated_records(&bytes);
            let parsed = super::owned_pcurve_from_record(&bytes, &records[0]);
            assert_eq!(parsed.map(|curve| curve.extrapolation_sites), expected);
            assert!(
                super::family_pcurves_from_records(&bytes, &records, ConsolidatedFamily::A)
                    .is_empty()
            );
        }
    }

    #[test]
    fn owned_pcurve_repeats_the_site_count_with_a_wide_channel_header() {
        for (repeated, accepted) in [(2_u16, true), (3, false)] {
            let mut bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
            let [low, high] = repeated.to_le_bytes();
            bytes.splice(31..33, [0x14, low, high]);
            let length = u32::try_from(bytes.len() - 8).expect("small frame");
            bytes[3..7].copy_from_slice(&length.to_le_bytes());

            let records = consolidated_records(&bytes);
            let parsed = super::owned_pcurve_from_record(&bytes, &records[0]);
            assert_eq!(parsed.map(|curve| curve.sites.len()), accepted.then_some(2));
        }
    }

    #[test]
    fn owned_pcurve_retains_odd_trailer_controls() {
        for control in [0x03, 0x25, 0xf9, 0x06] {
            let mut bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
            bytes.extend([0x01, control, 0x00]);
            let length = u32::try_from(bytes.len() - 8).expect("small frame");
            bytes[3..7].copy_from_slice(&length.to_le_bytes());
            let records = consolidated_records(&bytes);
            let parsed = super::owned_pcurve_from_record(&bytes, &records[0]);
            if control == 0x06 {
                assert!(parsed.is_none());
            } else {
                assert_eq!(
                    parsed.expect("odd owned control").tail,
                    [0x07, 0x01, control, 0x00]
                );
            }
            assert!(
                super::family_pcurves_from_records(&bytes, &records, ConsolidatedFamily::A)
                    .is_empty()
            );
        }
    }

    #[test]
    fn a5_pcurve_exposes_two_values_per_native_lane() {
        let bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        let records = consolidated_records(&bytes);
        let pcurves = super::family_pcurves_from_records(&bytes, &records, ConsolidatedFamily::A);
        let pcurve = &pcurves[0];
        let lanes = pcurve.native_lanes();
        assert_eq!(
            (lanes.0.len(), lanes.1.len(), lanes.2.len(), lanes.3.len()),
            (2, 2, 2, 2)
        );
    }
}

#[cfg(test)]
mod source_image_admission_tests {
    #[test]
    fn record_sources_reject_extents_from_a_larger_image() {
        let larger = [0_u8; 260];
        let short = [0xb2, 0x03, 0x24, 0xff, 0x01];
        let result =
            super::consolidated_records_in_sources(&short, [[super::SourceExtent::whole(&larger)]]);
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert!(super::parse_consolidated_record(&short, 0, larger.len()).is_none());
    }
}

#[cfg(test)]
mod width_token_admission_tests {
    #[test]
    fn consolidated_record_retains_an_immutable_width_coded_token() {
        for (marker, width, maximum) in [
            (0xb2, super::ConsolidatedFrameWidth::One, 0xff),
            (0xb3, super::ConsolidatedFrameWidth::Two, 0xffff),
            (0xb4, super::ConsolidatedFrameWidth::Three, 0xff_ffff),
        ] {
            let mut bytes = vec![marker, 3, 0x24, 0];
            bytes.resize(4 + usize::from(u8::from(width)), 0xff);
            let record =
                super::parse_consolidated_record(&bytes, 0, bytes.len()).expect("complete frame");
            assert_eq!(record.width(), width);
            assert_eq!(record.header_token(), maximum);
            assert!(super::WidthCodedToken::new(width, maximum + 1).is_err());
        }
    }

    #[test]
    fn width_four_allocation_record_continues_contiguous_journal() {
        // Record 0: B2 (width 1, token 0x05 -> increment 1), empty payload.
        // Record 1: B5 (width 4, flag 0x03, token 0x00_01_00_0c -> compact 3-byte u24 increment 0x00_01_00 = 256), empty payload.
        // Record 2: B2 (width 1, token 0x05 -> increment 1), empty payload.
        let bytes = [
            0xb2, 0x03, 0x24, 0x00, 0x05, 0xb5, 0x03, 0x20, 0x00, 0x0c, 0x00, 0x01, 0x00, 0xb2,
            0x03, 0x5f, 0x00, 0x05,
        ];
        let records = super::consolidated_records(&bytes);
        assert_eq!(records.len(), 3);
        assert_eq!(records[1].width(), super::ConsolidatedFrameWidth::Four);
        let journal = crate::families::consolidated::allocation::AllocationJournal::new(&records);
        assert_eq!(journal.absolute_address(0), Some(1));
        assert_eq!(journal.absolute_address(1), Some(257));
        assert_eq!(journal.absolute_address(2), Some(258));
    }

    #[test]
    fn spanning_record_skips_interior_marker_and_preserves_journal() {
        let mut bytes = vec![0xb2, 0x03, 0x20, 0x01, 0x05, 0xaa];
        let spanning_start = bytes.len();
        // Spanning A5_34 frame whose payload embeds a decoy `b2 03 5f 00 05` inside extent 1.
        let payload = [0x11, 0x22, 0xb2, 0x03, 0x5f, 0x00, 0x05, 0x33];
        bytes.extend_from_slice(&[0xa5, 0x03, 0x34]);
        bytes.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("payload len fits in u32")
                .to_le_bytes(),
        );
        bytes.push(0x05);
        bytes.extend_from_slice(&payload);
        let split = spanning_start + 9; // splits inside payload before decoy
        let after_start = bytes.len();
        bytes.extend_from_slice(&[0xb2, 0x03, 0x5f, 0x01, 0x05, 0xbb]);

        let sources = [vec![
            super::SourceExtent::within(&bytes, 0, split).expect("ext 0"),
            super::SourceExtent::within(&bytes, split, bytes.len()).expect("ext 1"),
        ]];
        let records =
            super::consolidated_records_in_sources(&bytes, &sources).expect("scan spanning source");
        assert_eq!(records.len(), 3);
        assert_eq!(records[1].source_range(), &(spanning_start..after_start));
        assert_eq!(
            records[1].payload_bytes(&bytes, &sources).as_deref(),
            Some(payload.as_slice())
        );
        let journal = crate::families::consolidated::allocation::AllocationJournal::new(&records);
        assert_eq!(journal.absolute_address(0), Some(1));
        assert_eq!(journal.absolute_address(1), Some(2));
        assert_eq!(journal.absolute_address(2), Some(3));
    }
}
