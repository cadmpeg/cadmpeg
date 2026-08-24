//! B2/B3/B4-family consolidated record vocabulary.
//!
//! Decodes analytic circle, cylinder, cone, and revolution charts, offset and
//! construction-use supports, class-`0x5e`/`0x61`/`0x62` owner and link records,
//! parameter-space packets, consolidated plane carriers, and consolidated UV
//! pcurves.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashSet};
use std::mem::size_of;

use crate::analytic::{periodic_angular_range_is_valid, sphere_angular_ranges_are_valid};
use crate::families::a5a8::records::FreeformSurface;
use crate::wire::bytes::persistent_ref;
use crate::wire::bytes::{
    allocation_ref, compact_int, f64_le, finite_f64_lane, read_f64_array, u32_le_24,
};
#[cfg(test)]
use crate::wire::records::{b_family_frames, consolidated_records};
use crate::wire::records::{
    b_family_frames_from_records, parse_consolidated_pcurve, ConsolidatedFamily, ConsolidatedFrame,
    ConsolidatedPcurve, ConsolidatedRecord,
};

const EPS_PLANE_DIRECTION_UNIT: f64 = 1e-9;
const EPS_PARAMETER_RANGE: f64 = 1e-6;
const EPS_SPATIAL_CIRCLE_UNIT: f64 = 1e-12;
const EPS_SPATIAL_CIRCLE_ORTHO: f64 = 1e-12;
const EPS_CONE_RANGE_START: f64 = 1e-12;
const EPS_CONE_DIRECTION_UNIT: f64 = 1e-9;
const EPS_CONE_FRAME_ORTHO: f64 = 1e-9;
const EPS_ANALYTIC_FRAME_UNIT: f64 = 1e-12;
const EPS_ANALYTIC_FRAME_ORTHO: f64 = 1e-12;
const EPS_ANALYTIC_AXIS_UNIT: f64 = 1e-9;
const EPS_ANALYTIC_AXIS_RANGE: f64 = 1e-9;

/// Offset-surface constructor stored in a `b2 03 31` support record or a
/// kind-`0x01` `b2 03 30` construction-use record.
#[derive(Debug, Clone)]
pub struct B2OffsetSupport {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced carrier-surface identifier.
    pub support_id: u32,
    /// Signed normal offset distance in millimetres.
    pub distance: f64,
    /// Carrier UV sub-domain `[u0, v0, u1, v1]`.
    pub domain: [f64; 4],
}

fn valid_offset_domain([u0, v0, u1, v1]: [f64; 4]) -> bool {
    [u0, v0, u1, v1].iter().all(|value| value.is_finite()) && u0 < u1 && v0 < v1
}

/// Parameter-space data stored in a `b2/b3/b4 03 18` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2ParameterPoint {
    /// Record byte offset.
    pub pos: usize,
    /// Exclusive end of the complete framed record.
    pub end: usize,
    /// Payload-layout discriminator (`0x12`, `0x1a`, or `0x2a`).
    pub layout: u8,
    /// First byte of the two-byte class-specific prefix.
    pub prefix: u8,
    /// Second byte of the two-byte class-specific prefix.
    pub control: u8,
    /// Layout-specific finite scalar lane.
    pub payload: B2ParameterPointPayload,
}

/// Layout-specific scalar lane of a class-`0x18` parameter-space record.
#[derive(Debug, Clone, PartialEq)]
pub enum B2ParameterPointPayload {
    /// Two-coordinate UV point (`L=0x12`).
    Uv {
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Host-chain station followed by UV (`L=0x1a`).
    StationUv {
        /// Host-chain axial boundary station.
        station: f64,
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Unsplit five-scalar layout (`L=0x2a`).
    FiveScalars {
        /// Stored scalar payload.
        values: [f64; 5],
    },
}

/// Structurally decoded payload of a consolidated class-`0x27` plane carrier.
///
/// The selector chooses the scalar layout. The trailing lanes are retained as
/// source scalars until their parameter-bound roles are established.
#[derive(Debug, Clone, PartialEq)]
pub enum B2PlaneCarrierPayload {
    /// Two-coordinate point, two-coordinate direction, and three tail scalars.
    PointDirection2 {
        /// In-plane point with the host-implied third coordinate omitted.
        point: [f64; 2],
        /// In-plane unit direction with its third component omitted.
        direction: [f64; 2],
        /// Complete trailing scalar lane.
        tail: [f64; 3],
    },
    /// Two-coordinate point, three-coordinate direction, and three tail scalars.
    PointDirection3 {
        /// In-plane point with the host-implied third coordinate omitted.
        point: [f64; 2],
        /// In-plane unit direction.
        direction: [f64; 3],
        /// Complete trailing scalar lane.
        tail: [f64; 3],
    },
    /// Two-coordinate point followed by four scalar values with no direction
    /// lane in this layout.
    PointTail {
        /// In-plane point with the host-implied third coordinate omitted.
        point: [f64; 2],
        /// Complete trailing scalar lane.
        tail: [f64; 4],
    },
    /// Finite scalar lane for a selector whose semantic layout is not yet
    /// established.
    ScalarLane {
        /// Complete selector-specific scalar lane in source order.
        values: Vec<f64>,
    },
}

/// One complete consolidated `b2/b3/b4 03 27` plane-carrier record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2PlaneCarrier {
    /// Record byte offset.
    pub pos: usize,
    /// Exclusive end of the complete framed record.
    pub end: usize,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent frame flag.
    pub flag: u8,
    /// Width-coded frame header token.
    pub header_token: u32,
    /// Second payload byte selecting the scalar layout.
    pub selector: u8,
    /// Selector-specific finite scalar payload.
    pub payload: B2PlaneCarrierPayload,
}

/// Persistent-tag reference list stored in a `b2/b3/b4 03 37` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2ReferenceList {
    /// Record byte offset.
    pub pos: usize,
    /// Compact persistent-tag references in serialization order.
    pub references: Vec<u32>,
}

/// Typed 62-byte tail of a fixed-nine class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct B2OwnerNumericTail {
    /// Five-byte class-specific header.
    pub header: [u8; 5],
    /// Lower coordinate pair of a strictly increasing binary64 box.
    pub lower: [f64; 2],
    /// Upper coordinate pair of a strictly increasing binary64 box.
    pub upper: [f64; 2],
    /// Three strictly increasing binary32 bounds in serialization order.
    pub bounds: [[f32; 2]; 3],
}

/// Nine-reference owner packet stored in a `b2/b3/b4 03 62` record with a
/// structurally decoded numeric tail.
#[derive(Debug, Clone, PartialEq)]
pub struct B2OwnerPacket {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Encoding selected by the first strong reference token.
    pub reference_encoding: B2OwnerReferenceEncoding,
    /// Nine compact persistent identities following the `0x89` count.
    pub references: [u32; 9],
    /// Fixed-width class-specific numeric tail.
    pub numeric_tail: B2OwnerNumericTail,
}

/// Count-framed class-`0x62` owner record with a class-specific tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2CountedOwner {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Persistent identities selected by the leading `0x80+n` count.
    pub references: Vec<u32>,
    /// Nonempty class-specific bytes after the reference lane.
    pub tail: Vec<u8>,
}

/// Reference dialect used by a nine-reference class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B2OwnerReferenceEncoding {
    /// Strong identities use `0x0a <u16le>` and weak identities use compact integers.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers and weak identities
    /// are raw one-byte values.
    WidthCodedStrong,
}

/// Count-prefixed class-`0x61` reference record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2Counted61 {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Compact values selected by the leading `0x80+n` count.
    pub references: Vec<u32>,
    /// Remaining class-specific bytes, including the terminal `0x03`.
    pub tail: Vec<u8>,
}

/// Long-form class-`0x61` record with a monotone u16 member lane.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Long61 {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Eight opaque bytes preceding the `0x06` list marker.
    pub prefix: [u8; 8],
    /// Strictly increasing little-endian u16 values.
    pub members: Vec<u16>,
    /// Five `0x0a <u16le>` persistent identities after delimiter `0xfe`.
    pub references: [u16; 5],
    /// Finite scalar preceding the terminal byte.
    pub scalar: f64,
}

/// Fixed-shape class-`0x5f` link record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B2Link5f {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Width-coded persistent target between `0x82` and the `03 05` tail.
    pub target: u32,
}

/// Adjacent class-`0x5f` link and class-`0x62` owner packet joined by their
/// allocation-successor identity.
#[derive(Debug, Clone, PartialEq)]
pub struct B2LinkedOwner {
    /// Fixed link immediately preceding the owner packet.
    pub link: B2Link5f,
    /// Nine-reference owner packet.
    pub owner: B2OwnerPacket,
}

/// Adjacent class-`0x5f` link and count-framed class-`0x62` owner joined by
/// the owner's allocation-successor identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2LinkedCountedOwner {
    /// Fixed link immediately preceding the owner packet.
    pub link: B2Link5f,
    /// Count-framed owner packet.
    pub owner: B2CountedOwner,
}

/// Cone-face chart descriptor stored in a `b2/b3/b4 03 3b` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2ConeFace {
    /// Record byte offset.
    pub pos: usize,
    /// Exclusive end of the complete framed record.
    pub end: usize,
    /// Complete reference-and-control program preceding the scalars.
    pub program: Vec<u8>,
    /// Stored angular chart scale.
    pub angular_scale: f64,
    /// Cone half-angle in radians.
    pub half_angle: f64,
}

/// Settled terminal sense code in a class-`0x06` consolidated use record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B2UseSense {
    /// Terminal byte `0x84`.
    Sense84,
    /// Terminal byte `0x88`.
    Sense88,
}

/// Byte-level metadata from a class-`0x06` consolidated use record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct B2UseMetadata {
    /// Record byte offset.
    pub pos: usize,
    /// Complete payload bytes.
    pub payload: Vec<u8>,
    /// Compact persistent references following the `0x80+n` count and
    /// preceding a settled terminal sense. `None` when the payload does not
    /// close under that grammar.
    pub references: Option<Vec<u32>>,
    /// Decoded terminal sense when the payload ends in `0x84` or `0x88`.
    pub sense: Option<B2UseSense>,
}

/// Byte-level metadata from a class-`0x5e` consolidated record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct B2EdgeMetadata {
    /// Record byte offset.
    pub pos: usize,
    /// Complete payload bytes.
    pub payload: Vec<u8>,
    /// Values carried by each `0x0a <u16le>` reference token.
    pub references: Vec<u16>,
}

/// Structurally decoded width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct B2EdgeNode {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token following the payload length.
    pub header_token: u32,
    /// Allocation-local curve-support reference terminating the use chain.
    pub curve_ref: u32,
    /// Native start-vertex identity.
    pub start_vertex_ref: u32,
    /// Native end-vertex identity.
    pub end_vertex_ref: u32,
    /// Allocation-local start-parameter selector.
    pub start_parameter_ref: u32,
    /// Allocation-local end-parameter selector.
    pub end_parameter_ref: u32,
    /// Terminal layout byte following the five references.
    pub tail: u8,
}

/// Decode class-`0x06` payloads and their settled terminal sense codes.
#[must_use]
#[cfg(test)]
pub fn b2_use_metadata(data: &[u8]) -> Vec<B2UseMetadata> {
    let records = consolidated_records(data);
    b2_use_metadata_from_records(data, &records)
}

pub(crate) fn b2_use_metadata_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2UseMetadata> {
    b_family_frames_from_records(records, 0x06)
        .into_iter()
        .map(|frame| {
            let payload = data[frame.payload..frame.end].to_vec();
            let sense = match payload.last() {
                Some(0x84) => Some(B2UseSense::Sense84),
                Some(0x88) => Some(B2UseSense::Sense88),
                _ => None,
            };
            let references = sense.and_then(|_| {
                let end = frame.end.checked_sub(1)?;
                let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
                let mut at = frame.payload + 1;
                let mut references = Vec::new();
                for _ in 0..count {
                    references.push(compact_int(data, &mut at)?);
                }
                (at == end).then_some(references)
            });
            B2UseMetadata {
                pos: frame.pos,
                payload,
                references,
                sense,
            }
        })
        .collect()
}

/// Decode class-`0x5e` payloads and their `0x0a <u16le>` reference tokens.
#[must_use]
#[cfg(test)]
pub fn b2_edge_metadata(data: &[u8]) -> Vec<B2EdgeMetadata> {
    b_family_frames(data, 0x5e)
        .into_iter()
        .map(|frame| {
            let payload = data[frame.payload..frame.end].to_vec();
            let mut references = Vec::new();
            let mut at = 0;
            while at < payload.len() {
                if let Some(value) = payload
                    .get(at)
                    .copied()
                    .filter(|byte| *byte == 0x0a)
                    .and_then(|_| View::u16_le_at(&payload, at + 1))
                {
                    references.push(value);
                    at += 3;
                } else {
                    at += 1;
                }
            }
            B2EdgeMetadata {
                pos: frame.pos,
                payload,
                references,
            }
        })
        .collect()
}

/// Decode length-closed `b2/b3/b4 03 5e` records containing one compact curve
/// reference, two persistent vertex references, two compact parameter
/// references, and one terminal byte.
#[must_use]
#[cfg(test)]
pub fn b2_edge_nodes(data: &[u8]) -> Vec<B2EdgeNode> {
    let records = consolidated_records(data);
    b2_edge_nodes_from_records(data, &records)
}

pub(crate) fn b2_edge_nodes_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2EdgeNode> {
    b_family_frames_from_records(records, 0x5e)
        .into_iter()
        .filter_map(|frame| {
            let token_start = frame.pos.checked_add(4)?;
            let mut token_end = token_start;
            let header_value = compact_int(data, &mut token_end)?;
            let canonical_token_width = match header_value {
                0..=0x3f => 1,
                0x40..=0xff => 2,
                _ => 3,
            };
            if token_end != frame.payload
                || frame.payload.checked_sub(token_start)? != canonical_token_width
            {
                return None;
            }
            let mut at = frame.payload;
            let curve_ref = compact_int(data, &mut at)?;
            let start_vertex_ref = allocation_ref(data, &mut at)?;
            let end_vertex_ref = allocation_ref(data, &mut at)?;
            let start_parameter_ref = compact_int(data, &mut at)?;
            let end_parameter_ref = compact_int(data, &mut at)?;
            let tail = *data.get(at)?;
            (at + 1 == frame.end && matches!(tail, 0x01 | 0x21 | 0x22 | 0x25 | 0x29 | 0x2a))
                .then_some(B2EdgeNode {
                    pos: frame.pos,
                    header_token: frame.header_token,
                    curve_ref,
                    start_vertex_ref,
                    end_vertex_ref,
                    start_parameter_ref,
                    end_parameter_ref,
                    tail,
                })
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 3b` cone-face descriptors.
#[must_use]
pub fn b2_cone_faces(data: &[u8]) -> Vec<B2ConeFace> {
    let mut faces = Vec::new();
    for pos in 0..data.len().saturating_sub(5) {
        let Some(width) = data[pos]
            .checked_sub(0xb1)
            .filter(|width| (1..=3).contains(width))
        else {
            continue;
        };
        if !matches!(data.get(pos + 1), Some(0x03 | 0x13 | 0x83))
            || data.get(pos + 2) != Some(&0x3b)
        {
            continue;
        }
        let payload = pos + 4 + usize::from(width);
        let Some(end) = payload.checked_add(usize::from(data[pos + 3])) else {
            continue;
        };
        if end > data.len() || end - payload < 0x20 {
            continue;
        }
        let header_token = data[pos + 4..payload]
            .iter()
            .enumerate()
            .fold(0u32, |value, (shift, byte)| {
                value | (u32::from(*byte) << (8 * shift))
            });
        let scalar_at = end - 16;
        let Some(program) = data.get(payload..scalar_at) else {
            continue;
        };
        let Some(angular_scale) = f64_le(data, scalar_at) else {
            continue;
        };
        let Some(half_angle) = f64_le(data, scalar_at + 8) else {
            continue;
        };
        if header_token == 5
            && program.first() == Some(&0x85)
            && program.ends_with(&[0x03, 0x11])
            && angular_scale.is_finite()
            && 0.0 < half_angle
            && half_angle < std::f64::consts::FRAC_PI_2
        {
            faces.push(B2ConeFace {
                pos,
                end,
                program: program.to_vec(),
                angular_scale,
                half_angle,
            });
        }
    }
    faces
}

/// Decode `b2/b3/b4 03 37` compact reference lists with their unit tail.
#[must_use]
#[cfg(test)]
pub fn b2_reference_lists(data: &[u8]) -> Vec<B2ReferenceList> {
    let records = consolidated_records(data);
    b2_reference_lists_from_records(data, &records)
}

pub(crate) fn b2_reference_lists_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2ReferenceList> {
    b_family_frames_from_records(records, 0x37)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5
                || !matches!(frame.end - frame.payload, 0x22 | 0x24 | 0x26)
                || f64_le(data, frame.end.checked_sub(8)?)? != 1.0
            {
                return None;
            }
            let refs_end = frame.end - 8;
            let mut at = frame.payload;
            let mut references = Vec::new();
            while at < refs_end {
                references.push(compact_int(data, &mut at)?);
            }
            (at == refs_end).then_some(B2ReferenceList {
                pos: frame.pos,
                references,
            })
        })
        .collect()
}

/// Decode class-`0x62` owner packets whose leading count fixes the persistent
/// reference lane and leaves a nonempty class-specific tail.
#[must_use]
#[cfg(test)]
pub fn b2_counted_owners(data: &[u8]) -> Vec<B2CountedOwner> {
    let records = consolidated_records(data);
    b2_counted_owners_from_records(data, &records)
}

pub(crate) fn b2_counted_owners_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2CountedOwner> {
    b_family_frames_from_records(records, 0x62)
        .into_iter()
        .filter_map(|frame| {
            let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
            if count == 0 {
                return None;
            }
            let mut at = frame.payload + 1;
            let references = (0..count)
                .map(|_| persistent_ref(data, &mut at))
                .collect::<Option<Vec<_>>>()?;
            (at < frame.end).then(|| B2CountedOwner {
                pos: frame.pos,
                header_token: frame.header_token,
                references,
                tail: data[at..frame.end].to_vec(),
            })
        })
        .collect()
}

/// Decode width-coded class-`0x62` owner packets whose counted references and
/// fixed numeric tail consume the complete frame.
#[must_use]
#[cfg(test)]
pub fn b2_owner_packets(data: &[u8]) -> Vec<B2OwnerPacket> {
    let records = consolidated_records(data);
    b2_owner_packets_from_records(data, &records)
}

pub(crate) fn b2_owner_packets_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2OwnerPacket> {
    b_family_frames_from_records(records, 0x62)
        .into_iter()
        .filter_map(|frame| {
            if data.get(frame.payload) != Some(&0x89) {
                return None;
            }
            let mut at = frame.payload + 1;
            let reference_encoding = if data.get(at) == Some(&0x0a) {
                B2OwnerReferenceEncoding::TaggedU16Strong
            } else {
                B2OwnerReferenceEncoding::WidthCodedStrong
            };
            let mut references = [0u32; 9];
            for (index, reference) in references.iter_mut().enumerate() {
                *reference = match (reference_encoding, index % 2) {
                    (B2OwnerReferenceEncoding::TaggedU16Strong, 0) => {
                        persistent_ref(data, &mut at)?
                    }
                    (B2OwnerReferenceEncoding::TaggedU16Strong, 1)
                    | (B2OwnerReferenceEncoding::WidthCodedStrong, 0) => {
                        compact_int(data, &mut at)?
                    }
                    (B2OwnerReferenceEncoding::WidthCodedStrong, 1) => {
                        let value = u32::from(*data.get(at)?);
                        at += 1;
                        value
                    }
                    _ => unreachable!(),
                };
            }
            let numeric_tail = b2_owner_numeric_tail(data.get(at..frame.end)?)?;
            Some(B2OwnerPacket {
                pos: frame.pos,
                header_token: frame.header_token,
                reference_encoding,
                references,
                numeric_tail,
            })
        })
        .collect()
}

fn b2_owner_numeric_tail(data: &[u8]) -> Option<B2OwnerNumericTail> {
    if data.len() != 62 {
        return None;
    }
    let header: [u8; 5] = data.get(..5)?.try_into().ok()?;
    if header[0] != 0x84 || !matches!(header[1], 0x41 | 0xc1) || header[4] != 0x0d {
        return None;
    }

    let values = read_f64_array::<4>(data, 5)?;
    let lower = [values[0], values[1]];
    let upper = [values[2], values[3]];
    if lower[0] >= upper[0] || lower[1] >= upper[1] {
        return None;
    }
    if data.get(37) != Some(&0x01) {
        return None;
    }
    let mut view = View::over_retained(data);
    view.seek(38)?;
    let mut bounds = [[0.0; 2]; 3];
    for bound in &mut bounds {
        bound[0] = view.f32_le()?;
        bound[1] = view.f32_le()?;
        if !bound[0].is_finite() || !bound[1].is_finite() || bound[0] >= bound[1] {
            return None;
        }
    }
    Some(B2OwnerNumericTail {
        header,
        lower,
        upper,
        bounds,
    })
}

/// Decode the count-prefixed class-`0x61` payload family. Long class-`0x61`
/// records without a leading count belong to a separate grammar and are not
/// returned.
#[must_use]
#[cfg(test)]
pub fn b2_counted_61(data: &[u8]) -> Vec<B2Counted61> {
    let records = consolidated_records(data);
    b2_counted_61_from_records(data, &records)
}

pub(crate) fn b2_counted_61_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Counted61> {
    b_family_frames_from_records(records, 0x61)
        .into_iter()
        .filter_map(|frame| {
            let count = usize::from(data.get(frame.payload)?.checked_sub(0x80)?);
            if count == 0 {
                return None;
            }
            let mut at = frame.payload + 1;
            let references = (0..count)
                .map(|_| compact_int(data, &mut at))
                .collect::<Option<Vec<_>>>()?;
            let tail = data.get(at..frame.end)?;
            if tail.is_empty() || tail.last() != Some(&0x03) {
                return None;
            }
            Some(B2Counted61 {
                pos: frame.pos,
                header_token: frame.header_token,
                references,
                tail: tail.to_vec(),
            })
        })
        .collect()
}

/// Decode the long class-`0x61` form. Its fixed 25-byte suffix determines the
/// monotone member-list boundary without searching for delimiter bytes.
#[must_use]
#[cfg(test)]
pub fn b2_long_61(data: &[u8]) -> Vec<B2Long61> {
    let records = consolidated_records(data);
    b2_long_61_from_records(data, &records)
}

pub(crate) fn b2_long_61_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Long61> {
    b_family_frames_from_records(records, 0x61)
        .into_iter()
        .filter_map(|frame| {
            let payload_len = frame.end.checked_sub(frame.payload)?;
            let delimiter = frame.end.checked_sub(25)?;
            if payload_len < 36
                || data.get(frame.payload + 8) != Some(&0x06)
                || data.get(delimiter) != Some(&0xfe)
                || (delimiter - (frame.payload + 9)) % 2 != 0
                || data.get(frame.end - 1) != Some(&0x03)
            {
                return None;
            }
            let prefix = data
                .get(frame.payload..frame.payload + 8)?
                .try_into()
                .ok()?;
            let mut members_view = View::over_retained(data.get(frame.payload + 9..delimiter)?);
            let mut members = Vec::new();
            while !members_view.is_empty() {
                members.push(members_view.u16_le()?);
            }
            if members.is_empty() || members.windows(2).any(|pair| pair[0] >= pair[1]) {
                return None;
            }
            let mut at = delimiter + 1;
            let mut references = [0u16; 5];
            for reference in &mut references {
                if data.get(at) != Some(&0x0a) {
                    return None;
                }
                *reference = View::u16_le_at(data, at + 1)?;
                at += 3;
            }
            let scalar = f64_le(data, at)?;
            if !scalar.is_finite() || at + 9 != frame.end {
                return None;
            }
            Some(B2Long61 {
                pos: frame.pos,
                header_token: frame.header_token,
                prefix,
                members,
                references,
                scalar,
            })
        })
        .collect()
}

/// Decode `82 <width-coded target> 03 05` class-`0x5f` links.
#[must_use]
#[cfg(test)]
pub fn b2_links_5f(data: &[u8]) -> Vec<B2Link5f> {
    let records = consolidated_records(data);
    b2_links_5f_from_records(data, &records)
}

pub(crate) fn b2_links_5f_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Link5f> {
    b_family_frames_from_records(records, 0x5f)
        .into_iter()
        .filter_map(|frame| {
            if data.get(frame.payload) != Some(&0x82) {
                return None;
            }
            let mut at = frame.payload + 1;
            let target = compact_int(data, &mut at)?;
            (at + 2 == frame.end && data.get(at..frame.end) == Some(&[0x03, 0x05])).then_some(
                B2Link5f {
                    pos: frame.pos,
                    header_token: frame.header_token,
                    target,
                },
            )
        })
        .collect()
}

/// Bind immediately adjacent `5f,62` records when the owner's ninth identity
/// is the checked successor of the link target.
#[must_use]
#[cfg(test)]
pub fn b2_linked_owners(data: &[u8]) -> Vec<B2LinkedOwner> {
    let records = consolidated_records(data);
    b2_linked_owners_from_records(data, &records)
}

pub(crate) fn b2_linked_owners_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2LinkedOwner> {
    let links = b2_links_5f_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let owners = b2_owner_packets_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    records
        .windows(2)
        .filter_map(|window| {
            let [link_record, owner_record] = window else {
                return None;
            };
            let link = links.get(&link_record.range.start)?;
            let owner = owners.get(&owner_record.range.start)?;
            (link.target.checked_add(1) == Some(owner.references[8])).then(|| B2LinkedOwner {
                link: *link,
                owner: owner.clone(),
            })
        })
        .collect()
}

/// Bind immediately adjacent `5f,62` records when the count-framed owner's
/// final identity is the checked successor of the link target.
#[must_use]
#[cfg(test)]
pub fn b2_linked_counted_owners(data: &[u8]) -> Vec<B2LinkedCountedOwner> {
    let records = consolidated_records(data);
    b2_linked_counted_owners_from_records(data, &records)
}

pub(crate) fn b2_linked_counted_owners_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2LinkedCountedOwner> {
    let links = b2_links_5f_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    let owners = b2_counted_owners_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<BTreeMap<_, _>>();
    records
        .windows(2)
        .filter_map(|window| {
            let [link_record, owner_record] = window else {
                return None;
            };
            let link = links.get(&link_record.range.start)?;
            let owner = owners.get(&owner_record.range.start)?;
            (link.target.checked_add(1) == owner.references.last().copied()).then(|| {
                B2LinkedCountedOwner {
                    link: *link,
                    owner: owner.clone(),
                }
            })
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 18` parameter-space records.
#[must_use]
#[cfg(test)]
pub fn b2_parameter_points(data: &[u8]) -> Vec<B2ParameterPoint> {
    let records = consolidated_records(data);
    b2_parameter_points_from_records(data, &records)
}

pub(crate) fn b2_parameter_points_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2ParameterPoint> {
    b_family_frames_from_records(records, 0x18)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 {
                return None;
            }
            let prefix = *data.get(frame.payload)?;
            if !matches!(prefix, 0x05 | 0x09 | 0x0d | 0x11) {
                return None;
            }
            let layout = u8::try_from(frame.end - frame.payload).ok()?;
            let control = *data.get(frame.payload + 1)?;
            let at = frame.payload + 2;
            let payload = match layout {
                0x12 => B2ParameterPointPayload::Uv {
                    uv: read_f64_array::<2>(data, at)?,
                },
                0x1a => {
                    let values = read_f64_array::<3>(data, at)?;
                    B2ParameterPointPayload::StationUv {
                        station: values[0],
                        uv: [values[1], values[2]],
                    }
                }
                0x2a => B2ParameterPointPayload::FiveScalars {
                    values: read_f64_array::<5>(data, at)?,
                },
                _ => return None,
            };
            let finite = match &payload {
                B2ParameterPointPayload::Uv { uv } => uv.iter().all(|v| v.is_finite()),
                B2ParameterPointPayload::StationUv { station, uv } => {
                    station.is_finite() && uv.iter().all(|v| v.is_finite())
                }
                B2ParameterPointPayload::FiveScalars { values } => {
                    values.iter().all(|v| v.is_finite())
                }
            };
            finite.then_some(B2ParameterPoint {
                pos: frame.pos,
                end: frame.end,
                layout,
                prefix,
                control,
                payload,
            })
        })
        .collect()
}

/// Decode complete consolidated class-`0x27` plane-carrier records.
#[must_use]
#[cfg(test)]
pub fn b2_plane_carriers(data: &[u8]) -> Vec<B2PlaneCarrier> {
    let records = consolidated_records(data);
    b2_plane_carriers_from_records(data, &records)
}

pub(crate) fn b2_plane_carriers_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2PlaneCarrier> {
    records
        .iter()
        .filter(|record| record.family == ConsolidatedFamily::B && record.class == 0x27)
        .filter_map(|record| {
            if !matches!(record.flag, 0x03 | 0x13 | 0x83) {
                return None;
            }
            let marker = *data.get(record.payload.start)?;
            let selector = *data.get(record.payload.start + 1)?;
            if marker != 0xb4 {
                return None;
            }
            let values = finite_f64_lane(data.get(record.payload.start + 2..record.payload.end)?)?;
            let payload = match selector {
                0xe4 => {
                    let values: [f64; 7] = values.try_into().ok()?;
                    B2PlaneCarrierPayload::PointDirection2 {
                        point: [values[0], values[1]],
                        direction: [values[2], values[3]],
                        tail: [values[4], values[5], values[6]],
                    }
                }
                0xc4 => {
                    let values: [f64; 8] = values.try_into().ok()?;
                    B2PlaneCarrierPayload::PointDirection3 {
                        point: [values[0], values[1]],
                        direction: [values[2], values[3], values[4]],
                        tail: [values[5], values[6], values[7]],
                    }
                }
                0xec => {
                    let values: [f64; 6] = values.try_into().ok()?;
                    B2PlaneCarrierPayload::PointTail {
                        point: [values[0], values[1]],
                        tail: [values[2], values[3], values[4], values[5]],
                    }
                }
                _ if !values.is_empty() => B2PlaneCarrierPayload::ScalarLane { values },
                _ => return None,
            };
            Some(B2PlaneCarrier {
                pos: record.range.start,
                end: record.range.end,
                width: record.width,
                flag: record.flag,
                header_token: record.header_token,
                selector,
                payload,
            })
        })
        .collect()
}

/// Recover the model-space plane carried by a direction-bearing class-`0x27`
/// layout. The omitted point coordinate is the host plane's third coordinate;
/// the direction-bearing layouts establish the positive in-plane axis and the
/// host Z direction establishes the second axis. The directionless `ec` layout
/// remains a retained native record until its axis rule is resolved.
pub(crate) fn b2_plane_geometry(carrier: &B2PlaneCarrier) -> Option<SurfaceGeometry> {
    let (point, direction, tail) = match &carrier.payload {
        B2PlaneCarrierPayload::PointDirection2 {
            point,
            direction,
            tail,
        } => (*point, [direction[0], direction[1], 0.0], *tail),
        B2PlaneCarrierPayload::PointDirection3 {
            point,
            direction,
            tail,
        } => (*point, *direction, *tail),
        B2PlaneCarrierPayload::PointTail { .. } | B2PlaneCarrierPayload::ScalarLane { .. } => {
            return None
        }
    };
    let u_axis = Vector3::new(direction[0], direction[1], direction[2]);
    let z_axis = Vector3::new(0.0, 0.0, 1.0);
    let normal = u_axis.cross(z_axis).unit()?;
    let valid_direction = (u_axis.norm() - 1.0).abs() <= EPS_PLANE_DIRECTION_UNIT
        && u_axis.z.abs() <= EPS_PLANE_DIRECTION_UNIT
        && direction.iter().all(|value| value.is_finite());
    let valid_tail =
        tail.iter().all(|value| value.is_finite()) && tail[0] > 0.0 && tail[1] < tail[2];
    (valid_direction && valid_tail).then_some(SurfaceGeometry::Plane {
        origin: Point3::new(point[0], point[1], 0.0),
        normal,
        u_axis,
    })
}

/// Decode class-`0x18` descriptors that prefix class-`0x25` edge definitions.
pub(crate) fn b2_class25_descriptors_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Class25Descriptor> {
    b_family_frames_from_records(records, 0x18)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 {
                return None;
            }
            let mut at = frame.payload;
            let record_id = compact_int(data, &mut at)?;
            let control = *data.get(at)?;
            at += 1;
            if !matches!(control, 0x02 | 0x0a) {
                return None;
            }
            let values = finite_f64_lane(data.get(at..frame.end)?)?;
            matches!(values.len(), 2 | 3).then_some(B2Class25Descriptor {
                pos: frame.pos,
                record_id,
                control,
                values,
            })
        })
        .collect()
}

/// Shared-edge parameter range stored in a `b2 03 23` packet.
#[derive(Debug, Clone)]
pub struct B2EdgeParameters {
    /// Record byte offset.
    pub pos: usize,
    /// Native shared-edge parameter range.
    pub range: [f64; 2],
    /// Shared-edge geometric tolerance.
    pub tolerance: f64,
}

/// Typed class-`0x18` descriptor immediately preceding a class-`0x25` edge.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Class25Descriptor {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded allocation identity.
    pub record_id: u32,
    /// Descriptor control byte (`0x02` or `0x0a`).
    pub control: u8,
    /// Complete finite scalar lane containing two or three values.
    pub values: Vec<f64>,
}

fn parameter_in_closed_range(value: f64, range: [f64; 2]) -> bool {
    let span = range[1] - range[0];
    if !span.is_finite() || span <= 0.0 {
        return false;
    }
    let tolerance = EPS_PARAMETER_RANGE * span;
    range[0] - tolerance <= value && value <= range[1] + tolerance
}

pub(crate) fn b2_cone_point(cone: &B2Cone, uv: [f64; 2]) -> Option<Point3> {
    if !parameter_in_closed_range(uv[1], cone.slant_range) {
        return None;
    }
    let phi = uv[0] / cone.angular_scale;
    let radial = [
        phi.cos() * cone.t1[0] + phi.sin() * cone.t2[0],
        phi.cos() * cone.t1[1] + phi.sin() * cone.t2[1],
        phi.cos() * cone.t1[2] + phi.sin() * cone.t2[2],
    ];
    let axial = cone.half_angle.cos();
    let transverse = cone.half_angle.sin();
    Some(Point3::new(
        cone.apex[0] + uv[1] * (axial * cone.axis[0] + transverse * radial[0]),
        cone.apex[1] + uv[1] * (axial * cone.axis[1] + transverse * radial[1]),
        cone.apex[2] + uv[1] * (axial * cone.axis[2] + transverse * radial[2]),
    ))
}

pub(crate) fn b2_cylinder_point(cylinder: &B2Cylinder, uv: [f64; 2]) -> Option<Point3> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = &cylinder.geometry
    else {
        return None;
    };
    if !parameter_in_closed_range(uv[0], cylinder.u_range)
        || !parameter_in_closed_range(uv[1], cylinder.v_range)
    {
        return None;
    }
    let angle = uv[0] / radius;
    let perpendicular = (*axis).cross(*ref_direction);
    Some(Point3::new(
        origin.x
            + uv[1] * axis.x
            + radius * (angle.cos() * ref_direction.x + angle.sin() * perpendicular.x),
        origin.y
            + uv[1] * axis.y
            + radius * (angle.cos() * ref_direction.y + angle.sin() * perpendicular.y),
        origin.z
            + uv[1] * axis.z
            + radius * (angle.cos() * ref_direction.z + angle.sin() * perpendicular.z),
    ))
}

pub(crate) fn point_distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Arc-length circle support stored in a `b2 03 19` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Circle {
    /// Record byte offset.
    pub pos: usize,
    /// Payload-layout discriminator (`0x32..=0x34`).
    pub layout: u8,
    /// Compact persistent record identifier.
    pub record_id: u32,
    /// Frame token following the record length.
    pub frame_token: u8,
    /// Two center coordinates in the host-implied carrier plane.
    pub center_pair: [f64; 2],
    /// Circle radius in millimetres.
    pub radius: f64,
    /// Arc-length parameter interval.
    pub range: [f64; 2],
    /// Whether the interval spans one complete circumference.
    pub full_circle: bool,
    /// Length-valued angular chart shift.
    pub chart_shift: f64,
}

/// One clamped rational NURBS curve stored in a `b2 03 16` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2NurbsCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded record token.
    pub header_token: u32,
    /// Exact neutral rational curve.
    pub geometry: NurbsCurve,
}

/// One spatial circle carrier stored in a `b2 03 0f` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2SpatialCircle {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded record token.
    pub header_token: u32,
    /// Circle centre.
    pub center: Point3,
    /// Unit circle-plane normal.
    pub axis: Vector3,
    /// Unit radial reference direction.
    pub ref_direction: Vector3,
    /// Positive radius in millimetres.
    pub radius: f64,
    /// Stored arc-length interval.
    pub range: [f64; 2],
    /// Stored chart shift.
    pub chart_shift: f64,
}

/// Decode length-closed `b2/b3/b4 03 0f` spatial circles.
#[must_use]
#[cfg(test)]
pub fn b2_spatial_circles(data: &[u8]) -> Vec<B2SpatialCircle> {
    let records = consolidated_records(data);
    b2_spatial_circles_from_records(data, &records)
}

pub(crate) fn b2_spatial_circles_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2SpatialCircle> {
    b_family_frames_from_records(records, 0x0f)
        .into_iter()
        .filter_map(|frame| parse_b2_spatial_circle(data, frame))
        .collect()
}

fn parse_b2_spatial_circle(data: &[u8], frame: ConsolidatedFrame) -> Option<B2SpatialCircle> {
    let values = read_f64_array::<14>(data, frame.payload)?;
    if frame.end.checked_sub(frame.payload)? != 14 * size_of::<f64>() {
        return None;
    }
    let center = Point3::new(values[0], values[1], values[2]);
    let ref_direction = Vector3::new(values[3], values[4], values[5]);
    let transverse = Vector3::new(values[6], values[7], values[8]);
    let ref_norm = ref_direction.norm();
    let transverse_norm = transverse.norm();
    let orthogonality = ref_direction.dot(transverse).abs();
    let axis = ref_direction.cross(transverse).unit()?;
    let radius = values[9];
    let range = [values[10], values[11]];
    if !values.iter().all(|value| value.is_finite())
        || (ref_norm - 1.0).abs() > EPS_SPATIAL_CIRCLE_UNIT
        || (transverse_norm - 1.0).abs() > EPS_SPATIAL_CIRCLE_UNIT
        || orthogonality > EPS_SPATIAL_CIRCLE_ORTHO
        || radius <= 0.0
        || range[0] >= range[1]
        || values[12].to_bits() != 1.0f64.to_bits()
    {
        return None;
    }
    Some(B2SpatialCircle {
        pos: frame.pos,
        header_token: frame.header_token,
        center,
        axis,
        ref_direction,
        radius,
        range,
        chart_shift: values[13],
    })
}

/// Decode length-closed `b2 03 16` rational NURBS curves.
///
/// The record stores one clamped span. The first compact integer is the degree,
/// so the control-point and weight cardinalities are both `degree + 1`. The two
/// knot limits occur twice and the second pair must reproduce the first pair.
#[must_use]
#[cfg(test)]
pub fn b2_nurbs_curves(data: &[u8]) -> Vec<B2NurbsCurve> {
    let records = consolidated_records(data);
    b2_nurbs_curves_from_records(data, &records)
}

pub(crate) fn b2_nurbs_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2NurbsCurve> {
    b_family_frames_from_records(records, 0x16)
        .into_iter()
        .filter_map(|frame| parse_b2_nurbs_curve(data, frame))
        .collect()
}

fn parse_b2_nurbs_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<B2NurbsCurve> {
    let mut at = frame.payload;
    let degree = compact_int(data, &mut at)?;
    let control_count = usize::try_from(degree.checked_add(1)?).ok()?;
    if !(1..=64).contains(&degree) || compact_int(data, &mut at)? != 2 {
        return None;
    }
    if data.get(at) != Some(&0x0c) {
        return None;
    }
    at += 1;
    let knot_start = f64_le(data, at)?;
    let knot_end = f64_le(data, at + 8)?;
    at += 16;
    if !knot_start.is_finite()
        || !knot_end.is_finite()
        || knot_start >= knot_end
        || compact_int(data, &mut at)? != 1
    {
        return None;
    }
    let control_points = (0..control_count)
        .map(|_| {
            let x = f64_le(data, at)?;
            let y = f64_le(data, at + 8)?;
            let z = f64_le(data, at + 16)?;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return None;
            }
            let point = Point3::new(x, y, z);
            at += 24;
            Some(point)
        })
        .collect::<Option<Vec<_>>>()?;
    let weights = (0..control_count)
        .map(|_| {
            let weight = f64_le(data, at)?;
            at += 8;
            (weight.is_finite() && weight > 0.0).then_some(weight)
        })
        .collect::<Option<Vec<_>>>()?;
    if compact_int(data, &mut at)? != 1 || compact_int(data, &mut at)? != 1 {
        return None;
    }
    let repeated_start = f64_le(data, at)?;
    let repeated_end = f64_le(data, at + 8)?;
    let scale = f64_le(data, at + 16)?;
    let offset = f64_le(data, at + 24)?;
    at += 32;
    if repeated_start.to_bits() != knot_start.to_bits()
        || repeated_end.to_bits() != knot_end.to_bits()
        || scale.to_bits() != 1.0f64.to_bits()
        || offset.to_bits() != 0.0f64.to_bits()
        || data.get(at..frame.end) != Some(&[0x00, 0x07])
    {
        return None;
    }
    let multiplicity = usize::try_from(degree.checked_add(1)?).ok()?;
    let mut knots = Vec::with_capacity(2 * multiplicity);
    knots.extend(std::iter::repeat_n(knot_start, multiplicity));
    knots.extend(std::iter::repeat_n(knot_end, multiplicity));
    Some(B2NurbsCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        geometry: NurbsCurve {
            degree,
            knots,
            control_points,
            weights: Some(weights),
            periodic: false,
        },
    })
}

/// Analytic cylinder support stored in a `b2 03 28` record.
#[derive(Debug, Clone)]
pub struct B2Cylinder {
    /// Record byte offset.
    pub pos: usize,
    /// Payload-layout discriminator (`0x52`, `0x5a`, or `0x62`).
    pub layout: u8,
    /// Frame token following the origin.
    pub frame_token: u8,
    /// Cylinder-axis origin.
    pub origin: [f64; 3],
    /// Cylinder radius.
    pub radius: f64,
    /// Decoded carrier.
    pub geometry: SurfaceGeometry,
    /// Arc-length circumferential range.
    pub u_range: [f64; 2],
    /// Axial range.
    pub v_range: [f64; 2],
    /// Stored planar vector for a range-origin `0x62` frame.
    pub stored_vector: Option<[f64; 2]>,
    /// Origin of the stored partial circumferential interval.
    pub range_origin: Option<f64>,
}

/// Slant-coordinate cone chart stored in a `b2 03 29` record.
#[derive(Debug, Clone)]
pub struct B2Cone {
    /// Record byte offset.
    pub pos: usize,
    /// Cone apex.
    pub apex: [f64; 3],
    /// First transverse unit direction.
    pub t1: [f64; 3],
    /// Second transverse unit direction.
    pub t2: [f64; 3],
    /// Cone-axis unit direction.
    pub axis: [f64; 3],
    /// Cone half-angle in radians.
    pub half_angle: f64,
    /// Scalar immediately preceding the active angular interval.
    pub pre_angular_range_scalar: f64,
    /// Active azimuth interval.
    pub angular_range: [f64; 2],
    /// Native slant-coordinate range.
    pub slant_range: [f64; 2],
    /// Divisor mapping the stored U coordinate to azimuth.
    pub angular_scale: f64,
    /// Full-turn azimuth chart domain.
    pub angular_domain: [f64; 2],
}

/// Axis-and-profile surface of revolution stored in a `b2 03 2d` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Revolution {
    /// Record byte offset.
    pub pos: usize,
    /// Reference-token dialect (`0x08` or `0x0a`).
    pub reference_token: u8,
    /// Stored profile allocation identity.
    pub profile_allocation_id: u16,
    /// Axis-frame origin.
    pub origin: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Revolution-axis direction.
    pub axis: [f64; 3],
    /// Stored angular parameter interval.
    pub angular_range: [f64; 2],
    /// Stored profile parameter interval.
    pub profile_range: [f64; 2],
    /// Positive angular chart scale.
    pub angular_scale: f64,
}

/// One revolution record whose profile interval identifies exactly one
/// consolidated circle record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2ResolvedRevolution {
    /// Ordinal among all decoded revolution records.
    pub revolution_index: usize,
    /// Surface-of-revolution record.
    pub revolution: B2Revolution,
    /// Unique profile circle with the same stored parameter interval.
    pub profile: B2Circle,
}

/// Metric line profile stored in a `b2/b3/b4 03 0e` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2LineProfile {
    /// Record byte offset.
    pub pos: usize,
    /// Stored line origin.
    pub origin: [f64; 3],
    /// Unit line direction.
    pub direction: [f64; 3],
    /// Increasing stored parameter interval.
    pub range: [f64; 2],
}

/// Radius-scaled sphere chart stored in a `b2 03 2a` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Sphere {
    /// Record byte offset.
    pub pos: usize,
    /// Sphere centre.
    pub center: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Sphere-axis unit direction.
    pub axis: [f64; 3],
    /// Sphere radius.
    pub radius: f64,
    /// Active azimuth interval.
    pub azimuth_range: [f64; 2],
    /// Active latitude interval.
    pub latitude_range: [f64; 2],
}

/// Doubly periodic torus chart stored in a `b2 03 2b` record.
#[derive(Debug, Clone, PartialEq)]
pub struct B2Torus {
    /// Record byte offset.
    pub pos: usize,
    /// Torus centre.
    pub center: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Torus-axis unit direction.
    pub axis: [f64; 3],
    /// Major radius.
    pub major_radius: f64,
    /// Minor radius.
    pub minor_radius: f64,
    /// Active major-angle interval.
    pub major_angular_range: [f64; 2],
    /// Full-turn major-angle chart domain.
    pub major_angular_domain: [f64; 2],
    /// Active minor-angle interval.
    pub minor_angular_range: [f64; 2],
    /// Full-turn minor-angle chart domain.
    pub minor_angular_domain: [f64; 2],
    /// Scale from major angle to stored U parameter.
    pub major_scale: f64,
    /// Scale from minor angle to stored V parameter.
    pub minor_scale: f64,
}

/// Constant `b2 03 65` separator preceding a typed group opener.
#[derive(Debug, Clone)]
#[cfg(test)]
pub struct B2GroupSeparator {
    /// Consolidated-frame header token.
    pub token: u32,
}

/// Typed group opener stored in a `b2 03 60` record.
#[derive(Debug, Clone)]
pub struct B2Group {
    /// Record byte offset.
    pub pos: usize,
    /// Compact group-type code; type `3` opens a cylinder chain.
    pub group_type: u32,
}

/// Construction-use wrapper stored in a `b2 03 30` record.
#[derive(Debug, Clone)]
pub struct B2ConstructionUse {
    /// Record byte offset.
    pub pos: usize,
    /// Referenced support identifier.
    pub support_id: u32,
    /// Signed wall or offset scalar.
    pub distance: f64,
    /// Construction-type discriminant.
    pub kind: u8,
    /// Carrier domain `[u0, v0, u1, v1]` for kind `0x01`.
    pub domain: Option<[f64; 4]>,
}

/// Cylinder frame following a type-3 `b2 03 60` group opener.
#[derive(Debug, Clone)]
pub struct B2EmbeddedCylinder {
    /// Group-opener byte offset.
    pub wrapper_pos: usize,
    /// Embedded frame byte offset, including its varying pre-byte.
    pub pos: usize,
    /// Compact embedded object identifier.
    pub object_id: u32,
    /// Decoded `0x5a` cylinder frame.
    pub cylinder: B2Cylinder,
}

/// Decode `0x5a` cylinder frames following type-3 `b2 03 60` group openers.
#[must_use]
#[cfg(test)]
pub fn b2_embedded_cylinders(data: &[u8]) -> Vec<B2EmbeddedCylinder> {
    let records = consolidated_records(data);
    b2_embedded_cylinders_from_records(data, &records)
}

pub(crate) fn b2_embedded_cylinders_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2EmbeddedCylinder> {
    let groups = b2_groups_from_records(data, records);
    let mut out = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if group.group_type != 3 {
            continue;
        }
        let wrapper_pos = group.pos;
        let end = groups.get(index + 1).map_or(data.len(), |next| next.pos);
        let mut search = wrapper_pos + 3;
        while search + 3 <= end {
            let Some(relative) = data[search..end]
                .windows(3)
                .position(|bytes| bytes == [0x03, 0x28, 0x5a])
            else {
                break;
            };
            let marker = search + relative;
            search = marker + 3;
            let mut payload = marker + 3;
            let Some(object_id) = compact_int(data, &mut payload) else {
                continue;
            };
            let Some(payload_end) = payload.checked_add(90) else {
                continue;
            };
            if payload_end > end {
                continue;
            }
            let mut standalone = vec![0xb2, 0x03, 0x28, 0x5a, 0];
            standalone.extend_from_slice(&data[payload..payload_end]);
            let Some(mut cylinder) = parse_b2_cylinder(
                &standalone,
                ConsolidatedFrame {
                    pos: 0,
                    payload: 5,
                    end: 95,
                    header_token: 0,
                },
            ) else {
                continue;
            };
            cylinder.pos = marker - 1;
            out.push(B2EmbeddedCylinder {
                wrapper_pos,
                pos: marker - 1,
                object_id,
                cylinder,
            });
        }
    }
    out
}

/// Decode `b2 03 30` construction-use wrappers.
#[must_use]
#[cfg(test)]
pub fn b2_construction_uses(data: &[u8]) -> Vec<B2ConstructionUse> {
    let records = consolidated_records(data);
    b2_construction_uses_from_records(data, &records)
}

pub(crate) fn b2_construction_uses_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2ConstructionUse> {
    let mut out = Vec::new();
    for frame in b_family_frames_from_records(records, 0x30) {
        let pos = frame.pos;
        let payload = frame.payload;
        if frame.header_token != 5 || data.get(payload) != Some(&0x05) {
            continue;
        }
        let (support_id, at) = match data.get(payload + 1) {
            Some(0x08) => {
                let Some(value) = View::u16_le_at(data, payload + 2) else {
                    continue;
                };
                (u32::from(value), payload + 4)
            }
            Some(0x0c) => {
                let Some(value) = u32_le_24(data, payload + 2) else {
                    continue;
                };
                (value, payload + 5)
            }
            _ => continue,
        };
        let Some(distance) = f64_le(data, at) else {
            continue;
        };
        let Some(&kind) = data.get(at + 8) else {
            continue;
        };
        let Some(fields) = read_f64_array::<4>(data, at + 9) else {
            continue;
        };
        if at + 41 != frame.end || !distance.is_finite() || fields.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let domain = (kind == 0x01)
            .then_some([fields[0], fields[2], fields[1], fields[3]])
            .filter(|domain| valid_offset_domain(*domain));
        out.push(B2ConstructionUse {
            pos,
            support_id,
            distance,
            kind,
            domain,
        });
    }
    out
}

/// Decode `b2 03 29` analytic cone charts.
#[must_use]
#[cfg(test)]
pub fn b2_cones(data: &[u8]) -> Vec<B2Cone> {
    let records = consolidated_records(data);
    b2_cones_from_records(data, &records)
}

pub(crate) fn b2_cones_from_records(data: &[u8], records: &[ConsolidatedRecord]) -> Vec<B2Cone> {
    let mut out = Vec::new();
    for frame in b_family_frames_from_records(records, 0x29) {
        let pos = frame.pos;
        let p = frame.payload;
        if frame.end - p != 0xb8 {
            continue;
        }
        let Some(values) = read_f64_array::<23>(data, p) else {
            continue;
        };
        let apex: [f64; 3] = values[0..3].try_into().expect("three apex values");
        let t1: [f64; 3] = values[3..6]
            .try_into()
            .expect("three first-direction values");
        let t2: [f64; 3] = values[6..9]
            .try_into()
            .expect("three second-direction values");
        let axis: [f64; 3] = values[9..12].try_into().expect("three axis values");
        let half_angle = values[12];
        let pre_angular_range_scalar = values[13];
        let angular_range = [values[14], values[15]];
        let mut slant_range = [values[16], values[17]];
        let angular_scale = values[18];
        let angular_domain = [values[21], values[22]];
        if slant_range[0].abs() <= EPS_CONE_RANGE_START {
            slant_range[0] = 0.0;
        }
        let unit = |v: [f64; 3]| {
            ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - 1.0).abs() < EPS_CONE_DIRECTION_UNIT
        };
        let cross = [
            t1[1] * t2[2] - t1[2] * t2[1],
            t1[2] * t2[0] - t1[0] * t2[2],
            t1[0] * t2[1] - t1[1] * t2[0],
        ];
        if values.iter().all(|value| value.is_finite())
            && unit(t1)
            && unit(t2)
            && unit(axis)
            && cross
                .iter()
                .zip(axis)
                .all(|(cross, axis)| (cross - axis).abs() <= EPS_CONE_FRAME_ORTHO)
            && 0.0 < half_angle
            && half_angle < std::f64::consts::FRAC_PI_2
            && periodic_angular_range_is_valid(angular_range, angular_domain)
            && 0.0 < angular_scale
            && values[19] == 1.0
            && values[20] == 0.0
            && 0.0 <= slant_range[0]
            && slant_range[0] < slant_range[1]
        {
            out.push(B2Cone {
                pos,
                apex,
                t1,
                t2,
                axis,
                half_angle,
                pre_angular_range_scalar,
                angular_range,
                slant_range,
                angular_scale,
                angular_domain,
            });
        }
    }
    out
}

/// Decode `b2 03 2d` axis-and-profile surfaces of revolution.
#[must_use]
#[cfg(test)]
pub fn b2_revolutions(data: &[u8]) -> Vec<B2Revolution> {
    let records = consolidated_records(data);
    b2_revolutions_from_records(data, &records)
}

pub(crate) fn b2_revolutions_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Revolution> {
    let mut out = Vec::new();
    for frame in b_family_frames_from_records(records, 0x2d) {
        let p = frame.payload;
        if frame.end - p != 0xae
            || !matches!(data.get(p), Some(0x08 | 0x0a))
            || data.get(p + 131..p + 133) != Some(&[0x05, 0x05])
            || f64_le(data, p + 141) != Some(1.0)
            || f64_le(data, p + 149) != Some(1.0)
            || f64_le(data, p + 157) != Some(0.0)
            || data.get(p + 165) != Some(&0x01)
        {
            continue;
        }
        let Some(profile_allocation_id) = View::u16_le_at(data, p + 1) else {
            continue;
        };
        let Some(axis_frame) = read_f64_array::<12>(data, p + 3) else {
            continue;
        };
        let Some(bounds) = read_f64_array::<4>(data, p + 99) else {
            continue;
        };
        let Some(angular_scale) = f64_le(data, p + 133) else {
            continue;
        };
        let Some(mean_angle_parameter) = f64_le(data, p + 166) else {
            continue;
        };
        let direction_x: [f64; 3] = axis_frame[3..6]
            .try_into()
            .expect("three first-direction values");
        let direction_y: [f64; 3] = axis_frame[6..9]
            .try_into()
            .expect("three second-direction values");
        let axis: [f64; 3] = axis_frame[9..12].try_into().expect("three axis values");
        let squared_length = |direction: [f64; 3]| {
            direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>()
        };
        let cross = [
            direction_x[1] * direction_y[2] - direction_x[2] * direction_y[1],
            direction_x[2] * direction_y[0] - direction_x[0] * direction_y[2],
            direction_x[0] * direction_y[1] - direction_x[1] * direction_y[0],
        ];
        if axis_frame
            .iter()
            .chain(&bounds)
            .chain(&[angular_scale, mean_angle_parameter])
            .any(|value| !value.is_finite())
            || profile_allocation_id == 0
            || angular_scale <= 0.0
            || bounds[2] >= bounds[3]
            || [direction_x, direction_y, axis]
                .into_iter()
                .any(|direction| (squared_length(direction) - 1.0).abs() > EPS_ANALYTIC_FRAME_UNIT)
            || cross
                .iter()
                .zip(axis)
                .any(|(cross, axis)| (cross - axis).abs() > EPS_ANALYTIC_FRAME_ORTHO)
            || bounds[0] / angular_scale != 0.5
            || (bounds[1] - bounds[0]) / angular_scale != std::f64::consts::TAU
            || mean_angle_parameter / angular_scale != std::f64::consts::PI + 0.5
        {
            continue;
        }
        out.push(B2Revolution {
            pos: frame.pos,
            reference_token: data[p],
            profile_allocation_id,
            origin: axis_frame[0..3].try_into().expect("three origin values"),
            direction_x,
            direction_y,
            axis,
            angular_range: [bounds[0], bounds[1]],
            profile_range: [bounds[2], bounds[3]],
            angular_scale,
        });
    }
    out
}

/// Bind revolution profiles by direct allocation identity, then by an exact,
/// unique stored parameter interval when no identity target is present.
#[must_use]
#[cfg(test)]
pub fn b2_resolved_revolutions(data: &[u8]) -> Vec<B2ResolvedRevolution> {
    let records = consolidated_records(data);
    b2_resolved_revolutions_from_records(data, &records)
}

pub(crate) fn b2_resolved_revolutions_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2ResolvedRevolution> {
    let circles = b2_circles_from_records(data, records);
    b2_revolutions_from_records(data, records)
        .into_iter()
        .enumerate()
        .filter_map(|(revolution_index, revolution)| {
            let identity_profiles = circles
                .iter()
                .filter(|circle| circle.record_id == u32::from(revolution.profile_allocation_id));
            let identity_profiles = identity_profiles.collect::<Vec<_>>();
            let profile = match identity_profiles.as_slice() {
                [profile]
                    if profile.range[0].to_bits() == revolution.profile_range[0].to_bits()
                        && profile.range[1].to_bits() == revolution.profile_range[1].to_bits() =>
                {
                    (*profile).clone()
                }
                [] => {
                    let mut interval_profiles = circles.iter().filter(|circle| {
                        circle.range[0].to_bits() == revolution.profile_range[0].to_bits()
                            && circle.range[1].to_bits() == revolution.profile_range[1].to_bits()
                    });
                    let profile = interval_profiles.next()?;
                    interval_profiles
                        .next()
                        .is_none()
                        .then(|| (*profile).clone())?
                }
                _ => return None,
            };
            Some(B2ResolvedRevolution {
                revolution_index,
                revolution,
                profile,
            })
        })
        .collect()
}

/// Decode exact B-family metric line profiles.
#[must_use]
#[cfg(test)]
pub fn b2_line_profiles(data: &[u8]) -> Vec<B2LineProfile> {
    let records = consolidated_records(data);
    b2_line_profiles_from_records(data, &records)
}

pub(crate) fn b2_line_profiles_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2LineProfile> {
    b_family_frames_from_records(records, 0x0e)
        .into_iter()
        .filter_map(|frame| {
            if frame.end - frame.payload != 9 * 8 {
                return None;
            }
            let values = read_f64_array::<9>(data, frame.payload)?;
            let direction: [f64; 3] = values[3..6].try_into().expect("three direction values");
            let squared_length = direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>();
            ((squared_length - 1.0).abs() <= EPS_ANALYTIC_FRAME_UNIT
                && values[6].to_bits() == 1.0_f64.to_bits()
                && values[7] < values[8])
                .then_some(B2LineProfile {
                    pos: frame.pos,
                    origin: values[0..3].try_into().expect("three origin values"),
                    direction,
                    range: [values[7], values[8]],
                })
        })
        .collect()
}

/// Decode `b2 03 2b` doubly periodic torus charts.
#[must_use]
#[cfg(test)]
pub fn b2_tori(data: &[u8]) -> Vec<B2Torus> {
    let records = consolidated_records(data);
    b2_tori_from_records(data, &records)
}

pub(crate) fn b2_tori_from_records(data: &[u8], records: &[ConsolidatedRecord]) -> Vec<B2Torus> {
    b_family_frames_from_records(records, 0x2b)
        .into_iter()
        .filter_map(|frame| {
            let p = frame.payload;
            (frame.end.checked_sub(p) == Some(200)).then_some(())?;
            let values = read_f64_array::<25>(data, p)?;
            let center: [f64; 3] = values[0..3].try_into().expect("three centre values");
            let direction_x: [f64; 3] = values[3..6]
                .try_into()
                .expect("three first-direction values");
            let direction_y: [f64; 3] = values[6..9]
                .try_into()
                .expect("three second-direction values");
            let axis: [f64; 3] = values[9..12].try_into().expect("three axis values");
            let major_radius = values[12];
            let minor_radius = values[13];
            let major_angular_range = [values[14], values[15]];
            let major_angular_domain = [values[16], values[17]];
            let minor_angular_range = [values[18], values[19]];
            let minor_angular_domain = [values[20], values[21]];
            let major_scale = values[22];
            let minor_scale = values[23];
            let dot = |first: [f64; 3], second: [f64; 3]| {
                first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
            };
            let unit = |value: [f64; 3]| (dot(value, value) - 1.0).abs() <= EPS_ANALYTIC_FRAME_UNIT;
            let cross = [
                direction_x[1] * direction_y[2] - direction_x[2] * direction_y[1],
                direction_x[2] * direction_y[0] - direction_x[0] * direction_y[2],
                direction_x[0] * direction_y[1] - direction_x[1] * direction_y[0],
            ];
            (values.iter().all(|value| value.is_finite())
                && unit(direction_x)
                && unit(direction_y)
                && unit(axis)
                && dot(direction_x, direction_y).abs() <= EPS_ANALYTIC_FRAME_ORTHO
                && dot(direction_x, axis).abs() <= EPS_ANALYTIC_FRAME_ORTHO
                && dot(direction_y, axis).abs() <= EPS_ANALYTIC_FRAME_ORTHO
                && cross
                    .iter()
                    .zip(axis)
                    .map(|(first, second)| (first - second).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    <= EPS_ANALYTIC_FRAME_ORTHO
                && major_radius > 0.0
                && minor_radius > 0.0
                && periodic_angular_range_is_valid(major_angular_range, major_angular_domain)
                && periodic_angular_range_is_valid(minor_angular_range, minor_angular_domain)
                && major_scale > 0.0
                && minor_scale > 0.0
                && values[24] == 0.0)
                .then_some(B2Torus {
                    pos: frame.pos,
                    center,
                    direction_x,
                    direction_y,
                    axis,
                    major_radius,
                    minor_radius,
                    major_angular_range,
                    major_angular_domain,
                    minor_angular_range,
                    minor_angular_domain,
                    major_scale,
                    minor_scale,
                })
        })
        .collect()
}

/// Decode `b2 03 2a` radius-scaled sphere charts.
#[must_use]
#[cfg(test)]
pub fn b2_spheres(data: &[u8]) -> Vec<B2Sphere> {
    let records = consolidated_records(data);
    b2_spheres_from_records(data, &records)
}

pub(crate) fn b2_spheres_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Sphere> {
    b_family_frames_from_records(records, 0x2a)
        .into_iter()
        .filter_map(|frame| {
            let p = frame.payload;
            (frame.end.checked_sub(p) == Some(152)).then_some(())?;
            let values = read_f64_array::<19>(data, p)?;
            let center: [f64; 3] = values[0..3].try_into().expect("three centre values");
            let stored_x: [f64; 3] = values[3..6]
                .try_into()
                .expect("three first-direction values");
            let stored_y: [f64; 3] = values[6..9]
                .try_into()
                .expect("three second-direction values");
            let stored_axis: [f64; 3] = values[9..12].try_into().expect("three axis values");
            let radius = values[12];
            let azimuth_range = [values[13], values[14]];
            let latitude_range = [values[15], values[16]];
            let construction_radius = values[17];
            let chart_origin = values[18];
            let scaled_length_is_radius = |value: [f64; 3]| {
                let length = value[0].hypot(value[1]).hypot(value[2]);
                length.is_finite() && ((length / radius) - 1.0).abs() <= EPS_ANALYTIC_FRAME_UNIT
            };
            (values.iter().all(|value| value.is_finite())
                && radius > 0.0
                && sphere_angular_ranges_are_valid(azimuth_range, latitude_range)
                && construction_radius.to_bits() == radius.to_bits()
                && chart_origin.to_bits()
                    == (radius
                        * ((azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI))
                        .to_bits()
                && scaled_length_is_radius(stored_x)
                && scaled_length_is_radius(stored_y)
                && scaled_length_is_radius(stored_axis))
            .then_some(())?;
            let direction_x = stored_x.map(|value| value / radius);
            let direction_y = stored_y.map(|value| value / radius);
            let axis = stored_axis.map(|value| value / radius);
            let cross = [
                direction_x[1] * direction_y[2] - direction_x[2] * direction_y[1],
                direction_x[2] * direction_y[0] - direction_x[0] * direction_y[2],
                direction_x[0] * direction_y[1] - direction_x[1] * direction_y[0],
            ];
            cross
                .iter()
                .zip(axis)
                .all(|(cross, axis)| (cross - axis).abs() <= EPS_ANALYTIC_FRAME_ORTHO)
                .then_some(B2Sphere {
                    pos: frame.pos,
                    center,
                    direction_x,
                    direction_y,
                    axis,
                    radius,
                    azimuth_range,
                    latitude_range,
                })
        })
        .collect()
}

/// Decode constant `b2 03 65` group separators.
#[must_use]
#[cfg(test)]
pub fn b2_group_separators(data: &[u8]) -> Vec<B2GroupSeparator> {
    b_family_frames(data, 0x65)
        .into_iter()
        .filter(|frame| data.get(frame.payload..frame.end) == Some(&[0x81, 0x03, 0x05, 0x0d]))
        .map(|frame| B2GroupSeparator {
            token: frame.header_token,
        })
        .collect()
}

/// Decode `b2 03 60` typed group openers.
#[must_use]
#[cfg(test)]
pub fn b2_groups(data: &[u8]) -> Vec<B2Group> {
    let records = consolidated_records(data);
    b2_groups_from_records(data, &records)
}

pub(crate) fn b2_groups_from_records(data: &[u8], records: &[ConsolidatedRecord]) -> Vec<B2Group> {
    b_family_frames_from_records(records, 0x60)
        .into_iter()
        .filter_map(|frame| {
            let mut at = frame.payload;
            if compact_int(data, &mut at)? != 32 {
                return None;
            }
            let group_type = compact_int(data, &mut at)?;
            (at == frame.end).then_some(B2Group {
                pos: frame.pos,
                group_type,
            })
        })
        .collect()
}

/// Convert a decoded B2 slant-coordinate cone chart to its equivalent IR carrier.
#[must_use]
pub fn b2_cone_geometry(cone: &B2Cone) -> SurfaceGeometry {
    let slant = cone.slant_range[0];
    let axial = slant * cone.half_angle.cos();
    SurfaceGeometry::Cone {
        origin: Point3::new(
            cone.apex[0] + axial * cone.axis[0],
            cone.apex[1] + axial * cone.axis[1],
            cone.apex[2] + axial * cone.axis[2],
        ),
        axis: Vector3::new(cone.axis[0], cone.axis[1], cone.axis[2]),
        ref_direction: Vector3::new(cone.t1[0], cone.t1[1], cone.t1[2]),
        radius: slant * cone.half_angle.sin(),
        ratio: 1.0,
        half_angle: cone.half_angle,
    }
}

/// Build the exact neutral carrier of a validated radius-scaled sphere chart.
#[must_use]
pub fn b2_sphere_geometry(sphere: &B2Sphere) -> SurfaceGeometry {
    SurfaceGeometry::Sphere {
        center: Point3::new(sphere.center[0], sphere.center[1], sphere.center[2]),
        axis: Vector3::new(sphere.axis[0], sphere.axis[1], sphere.axis[2]),
        ref_direction: Vector3::new(
            sphere.direction_x[0],
            sphere.direction_x[1],
            sphere.direction_x[2],
        ),
        radius: sphere.radius,
    }
}

/// Build the exact neutral carrier of a validated doubly periodic torus chart.
#[must_use]
pub fn b2_torus_geometry(torus: &B2Torus) -> SurfaceGeometry {
    SurfaceGeometry::Torus {
        center: Point3::new(torus.center[0], torus.center[1], torus.center[2]),
        axis: Vector3::new(torus.axis[0], torus.axis[1], torus.axis[2]),
        ref_direction: Vector3::new(
            torus.direction_x[0],
            torus.direction_x[1],
            torus.direction_x[2],
        ),
        major_radius: torus.major_radius,
        minor_radius: torus.minor_radius,
    }
}

/// Decode standalone `b2 03 28` analytic cylinder supports.
#[must_use]
#[cfg(test)]
pub fn b2_cylinders(data: &[u8]) -> Vec<B2Cylinder> {
    let records = consolidated_records(data);
    b2_cylinders_from_records(data, &records)
}

pub(crate) fn b2_cylinders_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Cylinder> {
    let embedded_offsets = b2_embedded_cylinders_from_records(data, records)
        .into_iter()
        .map(|embedded| embedded.pos)
        .collect::<HashSet<_>>();
    b_family_frames_from_records(records, 0x28)
        .into_iter()
        .filter_map(|frame| parse_b2_cylinder(data, frame))
        .filter(|cylinder| !embedded_offsets.contains(&cylinder.pos))
        .collect()
}

fn parse_b2_cylinder(data: &[u8], frame: ConsolidatedFrame) -> Option<B2Cylinder> {
    let pos = frame.pos;
    let layout = u8::try_from(frame.end.checked_sub(frame.payload)?).ok()?;
    let p = frame.payload;
    let origin_values = read_f64_array::<3>(data, p)?;
    let origin = Point3::new(origin_values[0], origin_values[1], origin_values[2]);
    let frame_token = *data.get(p + 24)?;
    match layout {
        0x5a => {
            if data.get(p + 89) != Some(&0x07) {
                return None;
            }
            let vector = read_f64_array::<2>(data, p + 25)?;
            let one = f64_le(data, p + 41)?;
            let radius = f64_le(data, p + 49)?;
            let u_range = read_f64_array::<2>(data, p + 57)?;
            let v_range = read_f64_array::<2>(data, p + 73)?;
            if one != 1.0
                || radius <= 0.0
                || origin_values.iter().any(|value| !value.is_finite())
                || vector.iter().any(|value| !value.is_finite())
                || u_range.iter().any(|value| !value.is_finite())
                || v_range.iter().any(|value| !value.is_finite())
                || (vector[0].hypot(vector[1]) - 1.0).abs() > EPS_ANALYTIC_AXIS_UNIT
                || u_range[0] >= u_range[1]
                || v_range[0] >= v_range[1]
                || !circle_range_is_full_turn(radius, u_range)
            {
                return None;
            }
            let axis = match frame_token {
                0x19 => Vector3::new(vector[0], vector[1], 0.0),
                0x1c => Vector3::new(vector[1], -vector[0], 0.0),
                _ => return None,
            };
            let ref_direction = Vector3::new(-axis.y, axis.x, 0.0);
            Some(B2Cylinder {
                pos,
                layout,
                frame_token,
                origin: origin_values,
                radius,
                geometry: SurfaceGeometry::Cylinder {
                    origin,
                    axis,
                    ref_direction,
                    radius,
                },
                u_range,
                v_range,
                stored_vector: None,
                range_origin: None,
            })
        }
        0x52 => {
            if frame_token != 0x1d
                || f64_le(data, p + 25)? != 1.0
                || f64_le(data, p + 33)? != 1.0
                || data.get(p + 81) != Some(&0x07)
            {
                return None;
            }
            let radius = f64_le(data, p + 41)?;
            let u_range = read_f64_array::<2>(data, p + 49)?;
            let v_range = read_f64_array::<2>(data, p + 65)?;
            if radius <= 0.0
                || origin_values.iter().any(|value| !value.is_finite())
                || u_range.iter().any(|value| !value.is_finite())
                || v_range.iter().any(|value| !value.is_finite())
                || u_range[0] >= u_range[1]
                || v_range[0] >= v_range[1]
                || !circle_range_is_full_turn(radius, u_range)
            {
                return None;
            }
            Some(B2Cylinder {
                pos,
                layout,
                frame_token,
                origin: origin_values,
                radius,
                geometry: SurfaceGeometry::Cylinder {
                    origin,
                    axis: Vector3::new(1.0, 0.0, 0.0),
                    ref_direction: Vector3::new(0.0, 1.0, 0.0),
                    radius,
                },
                u_range,
                v_range,
                stored_vector: None,
                range_origin: None,
            })
        }
        0x62 if frame_token == 0x0e && data.get(p + 89) == Some(&0x03) => {
            let vector = read_f64_array::<2>(data, p + 25)?;
            let one = f64_le(data, p + 41)?;
            let radius = f64_le(data, p + 49)?;
            let u_range = read_f64_array::<2>(data, p + 57)?;
            let v_range = read_f64_array::<2>(data, p + 73)?;
            let range_origin = f64_le(data, p + 90)?;
            let expected_range_origin = cylinder_range_origin(radius, u_range);
            if one != 1.0
                || radius <= 0.0
                || origin_values.iter().any(|value| !value.is_finite())
                || vector.iter().any(|value| !value.is_finite())
                || u_range.iter().any(|value| !value.is_finite())
                || v_range.iter().any(|value| !value.is_finite())
                || (vector[0].hypot(vector[1]) - 1.0).abs() > EPS_ANALYTIC_AXIS_UNIT
                || !range_origin.is_finite()
                || range_origin.to_bits() != expected_range_origin.to_bits()
                || u_range[0] >= u_range[1]
                || v_range[0] >= v_range[1]
                || !circle_range_is_within_full_turn(radius, u_range)
            {
                return None;
            }
            Some(B2Cylinder {
                pos,
                layout,
                frame_token,
                origin: origin_values,
                radius,
                geometry: SurfaceGeometry::Cylinder {
                    origin,
                    axis: Vector3::new(0.0, 1.0, 0.0),
                    ref_direction: Vector3::new(vector[0], 0.0, vector[1]),
                    radius,
                },
                u_range,
                v_range,
                stored_vector: Some(vector),
                range_origin: Some(range_origin),
            })
        }
        _ => None,
    }
}

pub(crate) fn cylinder_range_origin(radius: f64, u_range: [f64; 2]) -> f64 {
    (u_range[0] + u_range[1]) * 0.5 - std::f64::consts::PI * radius
}

/// Decode `b2 03 19` arc-length circle supports.
#[must_use]
#[cfg(test)]
pub fn b2_circles(data: &[u8]) -> Vec<B2Circle> {
    let records = consolidated_records(data);
    b2_circles_from_records(data, &records)
}

pub(crate) fn b2_circles_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2Circle> {
    let mut out = Vec::new();
    for frame in b_family_frames_from_records(records, 0x19) {
        let pos = frame.pos;
        if !(0x32..=0x34).contains(&(frame.end - frame.payload)) {
            continue;
        }
        let Ok(frame_token) = u8::try_from(frame.header_token) else {
            continue;
        };
        let mut at = frame.payload;
        let Some(record_id) = compact_int(data, &mut at) else {
            continue;
        };
        let Some(values) = read_f64_array::<5>(data, at) else {
            continue;
        };
        let values_end = at + 5 * size_of::<f64>();
        if values_end + 9 != frame.end || data.get(values_end) != Some(&0x01) {
            continue;
        }
        let Some(chart_shift) = f64_le(data, values_end + 1) else {
            continue;
        };
        let [c1, c2, radius, lo, hi] = values;
        if values.iter().all(|v| v.is_finite())
            && 0.0 < radius
            && c1.abs() <= 1e6
            && c2.abs() <= 1e6
            && hi > lo
            && chart_shift.is_finite()
        {
            out.push(B2Circle {
                pos,
                layout: (frame.end - frame.payload) as u8,
                record_id,
                frame_token,
                center_pair: [c1, c2],
                radius,
                range: [lo, hi],
                full_circle: circle_range_is_full_turn(radius, [lo, hi]),
                chart_shift,
            });
        }
    }
    out
}

pub(crate) fn circle_range_is_full_turn(radius: f64, range: [f64; 2]) -> bool {
    let relative_span = (range[1] - range[0]) / (std::f64::consts::TAU * radius);
    relative_span.is_finite() && (relative_span - 1.0).abs() < EPS_ANALYTIC_AXIS_RANGE
}

pub(crate) fn circle_range_is_within_full_turn(radius: f64, range: [f64; 2]) -> bool {
    let relative_span = (range[1] - range[0]) / (std::f64::consts::TAU * radius);
    relative_span.is_finite() && relative_span <= 1.0 + EPS_ANALYTIC_AXIS_RANGE
}

/// Decode structurally repeated `b2 03 23` edge-range packets.
#[must_use]
#[cfg(test)]
pub fn b2_edge_parameters(data: &[u8]) -> Vec<B2EdgeParameters> {
    let records = consolidated_records(data);
    b2_edge_parameters_from_records(data, &records)
}

pub(crate) fn b2_edge_parameters_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2EdgeParameters> {
    let mut out = Vec::new();
    for frame in b_family_frames_from_records(records, 0x23) {
        let pos = frame.pos;
        if frame.end - frame.payload != 0x4e {
            continue;
        }
        let Some(values) = read_f64_array::<9>(data, frame.payload + 6) else {
            continue;
        };
        if values.iter().all(|v| v.is_finite())
            && values[0] < values[1]
            && values[0] == values[3]
            && values[0] == values[6]
            && values[1] == values[4]
            && values[1] == values[7]
            && values[5] == 1.0
            && values[2] == values[8]
        {
            out.push(B2EdgeParameters {
                pos,
                range: [values[0], values[1]],
                tolerance: values[2],
            });
        }
    }
    out
}

/// Decode `b2 03 31` offset-surface constructors.
#[must_use]
#[cfg(test)]
pub fn b2_offset_supports(data: &[u8]) -> Vec<B2OffsetSupport> {
    let records = consolidated_records(data);
    b2_offset_supports_from_records(data, &records)
}

pub(crate) fn b2_offset_supports_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<B2OffsetSupport> {
    let mut offsets = b_family_frames_from_records(records, 0x31)
        .into_iter()
        .filter_map(|frame| {
            if frame.header_token != 5 {
                return None;
            }
            let length = frame.end - frame.payload;
            let (support_id, at) = match data.get(frame.payload) {
                Some(0x08) if length == 0x2b => (
                    u32::from(View::u16_le_at(data, frame.payload + 1)?),
                    frame.payload + 3,
                ),
                Some(0x0c) if length == 0x2c => {
                    (u32_le_24(data, frame.payload + 1)?, frame.payload + 4)
                }
                _ => return None,
            };
            let values = read_f64_array::<5>(data, at)?;
            let domain = [values[1], values[2], values[3], values[4]];
            (values[0].is_finite() && valid_offset_domain(domain)).then_some(B2OffsetSupport {
                pos: frame.pos,
                support_id,
                distance: values[0],
                domain,
            })
        })
        .collect::<Vec<_>>();
    offsets.extend(
        b2_construction_uses_from_records(data, records)
            .into_iter()
            .filter_map(|construction| {
                if construction.kind != 0x01 {
                    return None;
                }
                Some(B2OffsetSupport {
                    pos: construction.pos,
                    support_id: construction.support_id,
                    distance: construction.distance,
                    domain: construction.domain?,
                })
            }),
    );
    offsets.sort_unstable_by_key(|offset| offset.pos);
    offsets
}

/// Bind each offset constructor to the unique consolidated NURBS carrier whose
/// parameter domain contains the offset box and whose V-knot lane contains both
/// serialized V limits.
#[must_use]
pub fn offset_support_carriers(
    offsets: &[B2OffsetSupport],
    carriers: &[FreeformSurface],
) -> Vec<Option<usize>> {
    const RELATIVE_PARAMETER_TOLERANCE: f64 = 1e-3;
    offsets
        .iter()
        .map(|offset| {
            if !valid_offset_domain(offset.domain) {
                return None;
            }
            let [u0, v0, u1, v1] = offset.domain;
            let candidates = carriers
                .iter()
                .enumerate()
                .filter_map(|(index, carrier)| {
                    let SurfaceGeometry::Nurbs(surface) = &carrier.geometry else {
                        return None;
                    };
                    let u_min = *surface.u_knots.first()?;
                    let u_max = *surface.u_knots.last()?;
                    let v_min = *surface.v_knots.first()?;
                    let v_max = *surface.v_knots.last()?;
                    let u_span = u_max - u_min;
                    let v_span = v_max - v_min;
                    if !u_span.is_finite() || u_span <= 0.0 || !v_span.is_finite() || v_span <= 0.0
                    {
                        return None;
                    }
                    let u_tolerance = RELATIVE_PARAMETER_TOLERANCE * u_span;
                    let v_tolerance = RELATIVE_PARAMETER_TOLERANCE * v_span;
                    let contains = u0 >= u_min - u_tolerance
                        && u1 <= u_max + u_tolerance
                        && v0 >= v_min - v_tolerance
                        && v1 <= v_max + v_tolerance;
                    let has_v_limit = |limit: f64| {
                        surface
                            .v_knots
                            .iter()
                            .any(|knot| (*knot - limit).abs() <= v_tolerance)
                    };
                    (contains && has_v_limit(v0) && has_v_limit(v1)).then_some(index)
                })
                .collect::<Vec<_>>();
            <[usize; 1]>::try_from(candidates).ok().map(|[index]| index)
        })
        .collect()
}

/// Decode width-coded `b2/b3/b4 03 20` consolidated UV jets.
#[must_use]
#[cfg(test)]
pub fn b2_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    let records = consolidated_records(data);
    b2_pcurves_from_records(data, &records)
}

pub(crate) fn b2_pcurves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedPcurve> {
    b_family_frames_from_records(records, 0x20)
        .into_iter()
        .filter_map(|frame| parse_consolidated_pcurve(data, frame.pos, frame.payload, frame.end))
        .collect()
}
