//! A-family consolidated curve and surface record vocabulary.
//!
//! Decodes `a5`/`a8` NURBS surface carriers, common-form and consolidated
//! rolling-ball jets, guide-curve jets, and object-stream UV pcurves.

use crate::nurbs::{expand_knots, pole_count};
use crate::wire::bytes::{compact_int, f64_le, f64_point, read_f64_array, u32_le_24};
use crate::wire::records::{
    a_family_frames_from_records, consolidated_records, parse_consolidated_pcurve,
    ConsolidatedFrame, ConsolidatedPcurve, ConsolidatedRecord,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{
    knots_strictly_increasing, NurbsCurve, NurbsSurface, ProceduralSurfaceDefinition,
    RollingBallJetDerivative, RollingBallJetSite, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::ops::Range;

const EPS_GUIDE_DIRECTION_UNIT: f64 = 1.0e-9;
const EPS_ROLLING_BALL_RADIUS: f64 = 1.0e-9;
const EPS_ROLLING_BALL_ANGLE: f64 = 1.0e-9;

/// Native identity form of one decoded freeform surface carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreeformSurfaceIdentity {
    /// Common-form `a8 <flag> 34` carrier with an inline persistent object id.
    Object(u32),
    /// Consolidated `a5 03 34` carrier identified by its framed source offset.
    FrameOffset(usize),
}

/// A decoded common-form or consolidated freeform NURBS surface.
#[derive(Debug, Clone)]
pub struct FreeformSurface {
    /// Source offset of the framed record.
    pub pos: usize,
    /// Identity form carried by this storage family.
    pub identity: FreeformSurfaceIdentity,
    /// The decoded NURBS carrier.
    pub geometry: SurfaceGeometry,
}

/// Whether an `a8 <flag> 34` surface stores poles inline or in an external grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoleStorage {
    /// Pole and weight grid occupy the payload after the mode byte.
    Inline,
    /// The fixed 141-byte surface tail begins immediately after the mode byte.
    Elided,
}

impl FreeformSurface {
    /// Return the inline persistent object id when this is an A8 carrier.
    #[must_use]
    pub fn object_id(&self) -> Option<u32> {
        match self.identity {
            FreeformSurfaceIdentity::Object(object_id) => Some(object_id),
            FreeformSurfaceIdentity::FrameOffset(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
struct A8Frame {
    pos: usize,
    payload: usize,
    end: usize,
    object_id: u32,
}

#[derive(Clone, Copy)]
struct ObjectStreamFrame {
    pos: usize,
    payload: usize,
    end: usize,
    family: u8,
    class: u8,
    object_id: u32,
}

fn a8_frames(data: &[u8], class: u8) -> Vec<A8Frame> {
    let mut frames = Vec::new();
    let mut payload_end = None;
    let mut pos = 0usize;
    while pos + 11 <= data.len() {
        if let Some(end) = payload_end.filter(|end| pos < *end) {
            pos = end;
            continue;
        }
        if data[pos] != 0xa8 || !object_frame_flag(data[pos + 1]) {
            pos += 1;
            continue;
        }
        let Some(length) =
            View::u32_le_at(data, pos + 3).and_then(|value| usize::try_from(value).ok())
        else {
            pos += 1;
            continue;
        };
        let Some(end) = pos
            .checked_add(11)
            .and_then(|payload| payload.checked_add(length))
            .filter(|end| *end <= data.len())
        else {
            pos += 1;
            continue;
        };
        let Some(object_id) = View::u32_le_at(data, pos + 7) else {
            pos += 1;
            continue;
        };
        if data[pos + 2] == class {
            frames.push(A8Frame {
                pos,
                payload: pos + 11,
                end,
                object_id,
            });
        }
        payload_end = Some(end);
        pos += 1;
    }
    frames
}

fn object_frame_flag(flag: u8) -> bool {
    matches!(flag, 0x03 | 0x13 | 0x83)
}

fn object_stream_frame(data: &[u8], pos: usize) -> Option<ObjectStreamFrame> {
    if !object_frame_flag(*data.get(pos + 1)?) {
        return None;
    }
    let family = *data.get(pos)?;
    let class = *data.get(pos + 2)?;
    let (payload, length, object_id) = match family {
        0xb5 => (
            pos.checked_add(8)?,
            usize::from(*data.get(pos + 3)?),
            View::u32_le_at(data, pos + 4)?,
        ),
        0xa8 => (
            pos.checked_add(11)?,
            usize::try_from(View::u32_le_at(data, pos + 3)?).ok()?,
            View::u32_le_at(data, pos + 7)?,
        ),
        _ => return None,
    };
    let end = payload.checked_add(length)?;
    (end <= data.len()).then_some(ObjectStreamFrame {
        pos,
        payload,
        end,
        family,
        class,
        object_id,
    })
}

fn closed_a8_child_run(data: &[u8], start: usize, end: usize) -> bool {
    let mut at = start;
    while at < end {
        let Some(frame) = object_stream_frame(data, at) else {
            return false;
        };
        if frame.family != 0xb5 || frame.end > end {
            return false;
        }
        at = frame.end;
    }
    at == end
}

/// Return the start of a length-closed B5 child run owned by an A8 frame.
///
/// Common-form surface frames may place their child run after the complete
/// inline pole representation or after the fixed elided-pole tail. A marker
/// shaped byte sequence elsewhere in a surface payload is payload data and is
/// not a child run.
pub(crate) fn a8_nested_b5_run_start(
    data: &[u8],
    frame_start: usize,
    frame_end: usize,
) -> Option<usize> {
    let payload_start = frame_start.checked_add(11)?;
    if frame_end > data.len() || payload_start > frame_end {
        return None;
    }
    if payload_start < frame_end && closed_a8_child_run(data, payload_start, frame_end) {
        return Some(payload_start);
    }
    let frame = object_stream_frame(data, frame_start)?;
    if frame.family != 0xa8 || frame.class != 0x34 || frame.end != frame_end {
        return None;
    }
    let parsed = parse_a8_surface_header(
        data,
        A8Frame {
            pos: frame_start,
            payload: payload_start,
            end: frame_end,
            object_id: frame.object_id,
        },
    )?;
    let suffix_start = if parsed.header.pole_storage == PoleStorage::Elided {
        parsed.pole_start.checked_add(141)?
    } else {
        let poles = crate::nurbs_surface_control_count(
            usize::try_from(parsed.header.u_count).ok()?,
            usize::try_from(parsed.header.v_count).ok()?,
        )?;
        let pole_bytes = poles.checked_mul(24)?;
        let weight_bytes = if parsed.header.rational {
            poles.checked_mul(8)?
        } else {
            0
        };
        parsed
            .pole_start
            .checked_add(pole_bytes)?
            .checked_add(weight_bytes)?
    };
    let child_start = a8_surface_suffix_start(data, suffix_start, frame_end)?;
    (child_start < frame_end).then_some(child_start)
}

fn parse_a8_elided_surface_tail(data: &[u8], at: usize, v_knots: &[f64]) -> Option<usize> {
    let end = at.checked_add(141)?;
    let tail = data.get(at..end)?;
    if tail[0] != 0x05
        || tail[2] != 0x05
        || tail[1] % 4 != 1
        || tail[3] % 4 != 1
        || tail[68..71] != [0x01, 0x01, 0x01]
        || !tail[71..135].iter().all(|byte| *byte == 0)
        || tail[135..141] != [0x01, 0x00, 0x01, 0x00, 0x07, 0x07]
    {
        return None;
    }
    let read_f64 = |offset: usize| View::f64_le_at(tail, offset);
    let zero_u = read_f64(4)?;
    let positive_u = read_f64(12)?;
    let zero_v = read_f64(20)?;
    let v_span = read_f64(28)?;
    let one_u = read_f64(36)?;
    let zero_w = read_f64(44)?;
    let one_v = read_f64(52)?;
    let zero_x = read_f64(60)?;
    let (&v_last, &v_first) = v_knots.last().zip(v_knots.first())?;
    let expected_v_span = v_last - v_first;
    (zero_u == 0.0
        && positive_u.is_finite()
        && positive_u > 0.0
        && zero_v == 0.0
        && v_span.is_finite()
        && v_span > 0.0
        && v_span == expected_v_span
        && one_u == 1.0
        && zero_w == 0.0
        && one_v == 1.0
        && zero_x == 0.0)
        .then_some(end)
}

fn parse_surface_tail(data: &[u8], at: usize, end: usize) -> Option<usize> {
    let tail_len = end.checked_sub(at)?;
    let continuation_bytes = match tail_len {
        133 => 56,
        141 | 142 => 64,
        _ => return None,
    };
    let tail = data.get(at..end)?;
    if tail[0] != 0x05
        || tail[2] != 0x05
        || tail[1] % 4 != 1
        || tail[3] % 4 != 1
        || !matches!(tail[68..71], [0x01, 0x01, 0x01] | [0x05, 0x05, 0x01])
    {
        return None;
    }
    let parameters = read_f64_array::<8>(tail, 4)?;
    if parameters.iter().any(|value| !value.is_finite())
        || parameters[0] >= parameters[1]
        || parameters[2] >= parameters[3]
        || parameters[4] == 0.0
        || parameters[6] == 0.0
    {
        return None;
    }
    let continuation_start = 71;
    let continuation_end = continuation_start + continuation_bytes;
    let mut continuation_view =
        View::over_retained(tail.get(continuation_start..continuation_end)?);
    let mut continuation = Vec::new();
    while !continuation_view.is_empty() {
        continuation.push(continuation_view.f64_le()?);
    }
    if continuation.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let suffix = &tail[continuation_end..];
    let valid_suffix = match (tail_len, &tail[68..71]) {
        (133, [0x01, 0x01, 0x01]) => {
            continuation.iter().all(|value| *value == 0.0)
                && suffix == [0x01, 0x00, 0x01, 0x00, 0x07, 0x07]
        }
        (141, [0x01, 0x01, 0x01] | [0x05, 0x05, 0x01]) => matches!(
            suffix,
            [0x01, 0x00, 0x01, 0x00, 0x07, 0x07] | [0x09, 0x00, 0x09, 0x00, 0x07, 0x07]
        ),
        (142, [0x01, 0x01, 0x01] | [0x05, 0x05, 0x01]) => {
            suffix.len() == 7
                && suffix[0] % 4 == 1
                && suffix[1..4] == [0x00, 0x09, 0x01]
                && suffix[4] % 4 == 1
                && suffix[5..] == [0x07, 0x07]
        }
        _ => false,
    };
    valid_suffix.then_some(end)
}

fn valid_a5_surface_tail(data: &[u8], at: usize, end: usize) -> bool {
    parse_surface_tail(data, at, end).is_some()
}

fn a8_inline_surface_tail(data: &[u8], at: usize, end: usize) -> Option<usize> {
    for tail_len in [133, 141, 142] {
        let Some(tail_end) = at.checked_add(tail_len).filter(|tail_end| *tail_end <= end) else {
            continue;
        };
        if parse_surface_tail(data, at, tail_end).is_some()
            && closed_a8_child_run(data, tail_end, end)
        {
            return Some(tail_end);
        }
    }
    None
}

fn a8_surface_suffix_start(data: &[u8], at: usize, end: usize) -> Option<usize> {
    if closed_a8_child_run(data, at, end) {
        return Some(at);
    }
    a8_inline_surface_tail(data, at, end)
}

fn object_stream_frames(data: &[u8]) -> Vec<ObjectStreamFrame> {
    fn walk(
        data: &[u8],
        base: usize,
        admit_a8: bool,
        admit_b5: bool,
        frames: &mut Vec<ObjectStreamFrame>,
    ) {
        let mut pos = 0usize;
        while pos + 8 <= data.len() {
            let Some(frame) = object_stream_frame(data, pos) else {
                pos += 1;
                continue;
            };
            match frame.family {
                0xa8 if admit_a8 => {
                    frames.push(ObjectStreamFrame {
                        pos: base + frame.pos,
                        payload: base + frame.payload,
                        end: base + frame.end,
                        ..frame
                    });
                    walk(
                        &data[frame.payload..frame.end],
                        base + frame.payload,
                        false,
                        admit_b5,
                        frames,
                    );
                    pos = frame.end;
                }
                0xb5 if admit_b5 => {
                    frames.push(ObjectStreamFrame {
                        pos: base + frame.pos,
                        payload: base + frame.payload,
                        end: base + frame.end,
                        ..frame
                    });
                    pos = frame.end;
                }
                _ => pos += 1,
            }
        }
    }

    let mut frames = Vec::new();
    walk(data, 0, true, true, &mut frames);
    frames
}

/// Parameter lattice decoded from an `a8 <flag> 34` surface record independently
/// of its pole representation.
#[derive(Debug, Clone, PartialEq)]
pub struct A8SurfaceHeader {
    /// Source offset of the framed record.
    pub pos: usize,
    /// Inline persistent object id.
    pub object_id: u32,
    /// U degree.
    pub u_degree: u32,
    /// V degree.
    pub v_degree: u32,
    /// Distinct U knots.
    pub u_distinct_knots: Vec<f64>,
    /// Distinct V knots.
    pub v_distinct_knots: Vec<f64>,
    /// U multiplicities corresponding to `u_distinct_knots`.
    pub u_multiplicities: Vec<u32>,
    /// V multiplicities corresponding to `v_distinct_knots`.
    pub v_multiplicities: Vec<u32>,
    /// Derived U pole count.
    pub u_count: u32,
    /// Derived V pole count.
    pub v_count: u32,
    /// Whether the record selects rational weights.
    pub rational: bool,
    /// Whether poles occupy the payload or an external grid.
    pub pole_storage: PoleStorage,
}

#[derive(Debug, Clone)]
/// Degree-5 UV jet stored in an `a8 <flag> 20` object record.
pub struct A8Pcurve {
    /// Record byte offset.
    #[cfg(test)]
    pub pos: usize,
    /// Inline object identifier.
    pub object_id: u32,
    /// Referenced support-surface object identifier.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Stored UV-jet channel-mode byte.
    #[cfg(test)]
    pub mode: u8,
    /// UV positions at the knot sites.
    pub points: Vec<[f64; 2]>,
    /// UV first derivatives at the knot sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// UV second derivatives at the knot sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native parameter range.
    pub range: [f64; 2],
}

/// Decode framed `a5 03 20` consolidated UV jets.
#[must_use]
#[cfg(test)]
pub fn a5_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    let records = consolidated_records(data);
    a5_pcurves_from_records(data, &records)
}

pub(crate) fn a5_pcurves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<ConsolidatedPcurve> {
    a_family_frames_from_records(records, 0x20)
        .into_iter()
        .filter_map(|frame| parse_consolidated_pcurve(data, frame.pos, frame.payload, frame.end))
        .collect()
}

/// One knot-site value in an `a5 03 32` rolling-ball program.
#[derive(Debug, Clone, PartialEq)]
pub struct RollingBallSite {
    /// First limiting curve point.
    pub limit1: [f64; 3],
    /// Second limiting curve point.
    pub limit2: [f64; 3],
    /// Rolling-ball centre.
    pub center: [f64; 3],
    /// Stored opening angle.
    pub theta: f64,
    /// Radius derived from centre to either limit.
    pub radius: f64,
}

/// Consolidated degree-5 rolling-ball jet.
#[derive(Debug, Clone)]
pub struct A5FreeformCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Schema token immediately before the payload.
    pub header_token: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct knots.
    pub knots: Vec<f64>,
    /// Position channels at each knot.
    pub sites: Vec<RollingBallSite>,
    /// Ten first-derivative channels per knot.
    pub first_derivatives: Vec<[f64; 10]>,
    /// Ten second-derivative channels per knot.
    pub second_derivatives: Vec<[f64; 10]>,
}

/// Lower either limiting locus of a complete rolling-ball jet to its exact
/// degree-5 NURBS representation.
pub(crate) fn rolling_ball_limit_curve(
    jet: &A5FreeformCurve,
    second_limit: bool,
) -> Option<NurbsCurve> {
    let offset = usize::from(second_limit) * 3;
    let positions = jet
        .sites
        .iter()
        .map(|site| {
            if second_limit {
                site.limit2
            } else {
                site.limit1
            }
        })
        .collect::<Vec<_>>();
    let first = jet
        .first_derivatives
        .iter()
        .map(|values| [values[offset], values[offset + 1], values[offset + 2]])
        .collect::<Vec<_>>();
    let second = jet
        .second_derivatives
        .iter()
        .map(|values| [values[offset], values[offset + 1], values[offset + 2]])
        .collect::<Vec<_>>();
    let (knots, control_points) =
        crate::nurbs::quintic_jet_bspline3(jet.degree, &jet.knots, &positions, &first, &second)?;
    NurbsCurve::new(
        jet.degree,
        knots,
        control_points
            .into_iter()
            .map(|point| Point3::new(point[0], point[1], point[2]))
            .collect(),
        None,
        false,
    )
    .ok()
}

/// One position and unit reference direction in an `a5/a6/a7 03 39` jet.
#[derive(Debug, Clone, PartialEq)]
pub struct GuideCurveSite {
    /// Guide-curve point.
    pub point: [f64; 3],
    /// Unit direction from the first stored triple to the second.
    pub direction: [f64; 3],
}

/// Width-coded guide-curve and reference-direction jet.
#[derive(Debug, Clone)]
pub struct A5GuideCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Position and unit-direction values at the knot sites.
    pub sites: Vec<GuideCurveSite>,
    /// Six first-derivative channels per site.
    pub first_derivatives: Vec<[f64; 6]>,
    /// Six second-derivative channels per site.
    pub second_derivatives: Vec<[f64; 6]>,
}

/// One non-rational degree-5 NURBS curve stored in an `a5 13 16` frame.
#[derive(Debug, Clone, PartialEq)]
pub struct A5NurbsCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Width-coded record token.
    pub header_token: u32,
    /// Exact neutral curve.
    pub geometry: NurbsCurve,
}

/// Decode length-closed `a5/a6/a7 13 16` non-rational NURBS curves.
#[must_use]
#[cfg(test)]
pub fn a5_nurbs_curves(data: &[u8]) -> Vec<A5NurbsCurve> {
    let records = consolidated_records(data);
    a5_nurbs_curves_from_records(data, &records)
}

pub(crate) fn a5_nurbs_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<A5NurbsCurve> {
    a_family_frames_from_records(records, 0x16)
        .into_iter()
        .filter_map(|frame| parse_a5_nurbs_curve(data, frame))
        .collect()
}

fn parse_a5_nurbs_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5NurbsCurve> {
    let mut at = frame.payload;
    let degree = compact_int(data, &mut at)?;
    let knot_count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    if degree != 5 || knot_count < 2 || data.get(at) != Some(&0x0c) {
        return None;
    }
    at += 1;
    let control_count = 6usize.checked_add(knot_count.checked_sub(2)?.checked_mul(3)?)?;
    let known_bytes = knot_count
        .checked_mul(8)?
        .checked_add(control_count.checked_mul(24)?)?
        .checked_add(36)?;
    if at.checked_add(known_bytes)? > frame.end {
        return None;
    }
    let mut distinct_knots = Vec::with_capacity(knot_count);
    for _ in 0..knot_count {
        distinct_knots.push(f64_le(data, at)?);
        at += 8;
    }
    if distinct_knots.iter().any(|knot| !knot.is_finite())
        || !knots_strictly_increasing(&distinct_knots)
        || data.get(at) != Some(&0x01)
    {
        return None;
    }
    at += 1;
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
    if compact_int(data, &mut at)? != 1 || compact_int(data, &mut at)? != 2 {
        return None;
    }
    let range_origin = f64_le(data, at)?;
    let repeated_end = f64_le(data, at + 8)?;
    let scale = f64_le(data, at + 16)?;
    let offset = f64_le(data, at + 24)?;
    at += 32;
    if range_origin.to_bits() != 0.0f64.to_bits()
        || repeated_end.to_bits() != distinct_knots.last()?.to_bits()
        || scale.to_bits() != 1.0f64.to_bits()
        || offset.to_bits() != 0.0f64.to_bits()
        || data.get(at..frame.end) != Some(&[0x00, 0x07])
    {
        return None;
    }
    let mut knots = Vec::with_capacity(control_count + usize::try_from(degree).ok()? + 1);
    for (index, knot) in distinct_knots.into_iter().enumerate() {
        let multiplicity = if index == 0 || index + 1 == knot_count {
            6
        } else {
            3
        };
        knots.extend(std::iter::repeat_n(knot, multiplicity));
    }
    Some(A5NurbsCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        geometry: NurbsCurve::new(degree, knots, control_points, None, false).ok()?,
    })
}

/// Decode `a5/a6/a7 03 39` guide-curve and unit-direction jets.
#[must_use]
#[cfg(test)]
pub fn a5_guide_curves(data: &[u8]) -> Vec<A5GuideCurve> {
    let records = consolidated_records(data);
    a5_guide_curves_from_records(data, &records)
}

pub(crate) fn a5_guide_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<A5GuideCurve> {
    a_family_frames_from_records(records, 0x39)
        .into_iter()
        .filter_map(|frame| parse_a5_guide_curve(data, frame))
        .collect()
}

fn parse_a5_guide_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5GuideCurve> {
    let mut at = frame.payload;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count
        || count < 2
        || !(1..=9).contains(&degree)
    {
        return None;
    }
    at = consume_array_marker(data, at)?;
    let block_bytes = count.checked_mul(48)?;
    let known_bytes = count
        .checked_mul(8)?
        .checked_add(block_bytes.checked_mul(3)?)?
        .checked_add(48)?;
    if at.checked_add(known_bytes)? > frame.end {
        return None;
    }
    let knots = f64_values(data, &mut at, count, frame.end)?;
    if knots.iter().any(|knot| !knot.is_finite()) || !knots_strictly_increasing(&knots) {
        return None;
    }
    if at
        .checked_add(block_bytes.checked_mul(3)?)?
        .checked_add(48)?
        != frame.end
    {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 6]>> {
        (0..count)
            .map(|site| {
                let values = read_f64_array::<6>(data, start + site * 48)?;
                values
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites: Option<Vec<_>> = positions
        .into_iter()
        .map(|value| {
            let point = [value[0], value[1], value[2]];
            let direction = [
                value[3] - value[0],
                value[4] - value[1],
                value[5] - value[2],
            ];
            let length =
                (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
            ((length - 1.0).abs() < EPS_GUIDE_DIRECTION_UNIT)
                .then_some(GuideCurveSite { point, direction })
        })
        .collect();
    Some(A5GuideCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        degree,
        knots,
        sites: sites?,
        first_derivatives,
        second_derivatives,
    })
}

/// Common-form degree-5 rolling-ball jet stored in an `a8 <flag> 32` object record.
#[derive(Debug, Clone, PartialEq)]
pub struct A8FreeformCurve {
    /// Record byte offset.
    pub pos: usize,
    /// Inline persistent object identifier.
    pub object_id: u32,
    /// Parametric degree.
    pub degree: u32,
    /// Distinct parameter knots.
    pub knots: Vec<f64>,
    /// Multiplicity for each distinct knot.
    pub multiplicities: Vec<u32>,
    /// Position channels at each knot.
    pub sites: Vec<RollingBallSite>,
    /// Ten first-derivative channels per knot.
    pub first_derivatives: Vec<[f64; 10]>,
    /// Ten second-derivative channels per knot.
    pub second_derivatives: Vec<[f64; 10]>,
    /// Bytes following the three jet blocks inside the payload.
    pub tail_len: usize,
}

/// Convert a complete common-form rolling-ball jet to its exact neutral
/// procedural carrier.
pub(crate) fn rolling_ball_jet_definition(
    jet: &A8FreeformCurve,
) -> Option<ProceduralSurfaceDefinition> {
    if jet.degree != 5
        || jet.sites.len() != jet.knots.len()
        || jet.first_derivatives.len() != jet.knots.len()
        || jet.second_derivatives.len() != jet.knots.len()
        || jet.multiplicities.len() != jet.knots.len()
    {
        return None;
    }
    let derivative = |values: [f64; 10]| RollingBallJetDerivative {
        first_limit: Vector3::new(values[0], values[1], values[2]),
        second_limit: Vector3::new(values[3], values[4], values[5]),
        center: Vector3::new(values[6], values[7], values[8]),
        angle: values[9],
    };
    let sites = jet
        .sites
        .iter()
        .zip(&jet.first_derivatives)
        .zip(&jet.second_derivatives)
        .map(|((site, first), second)| RollingBallJetSite {
            first_limit: Point3::new(site.limit1[0], site.limit1[1], site.limit1[2]),
            second_limit: Point3::new(site.limit2[0], site.limit2[1], site.limit2[2]),
            center: Point3::new(site.center[0], site.center[1], site.center[2]),
            angle: site.theta,
            first_derivative: derivative(*first),
            second_derivative: derivative(*second),
        })
        .collect();
    Some(ProceduralSurfaceDefinition::RollingBallJet {
        degree: jet.degree,
        multiplicities: jet.multiplicities.clone(),
        knots: jet.knots.clone(),
        sites,
    })
}

/// Decode framed `a8 <flag> 32` common-form rolling-ball jet records.
#[must_use]
pub fn a8_freeform_curves(data: &[u8]) -> Vec<A8FreeformCurve> {
    a8_frames(data, 0x32)
        .into_iter()
        .filter_map(|frame| parse_a8_curve(data, frame))
        .collect()
}

fn parse_a8_curve(data: &[u8], frame: A8Frame) -> Option<A8FreeformCurve> {
    let A8Frame {
        pos,
        payload,
        end,
        object_id,
    } = frame;
    let mut at = payload.checked_add(1)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count || count < 2 || degree != 5 {
        return None;
    }
    at = at.checked_add(if data.get(at) == Some(&0x08) { 2 } else { 1 })?;
    let knot_bytes = count.checked_mul(8)?;
    let block_bytes = count.checked_mul(80)?;
    let known_bytes = knot_bytes
        .checked_add(count)?
        .checked_add(block_bytes.checked_mul(3)?)?
        .checked_add(59)?;
    if at.checked_add(known_bytes)? > end {
        return None;
    }
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        knots.push(f64_le(data, at)?);
        at += 8;
    }
    let mut multiplicities = Vec::with_capacity(count);
    for _ in 0..count {
        multiplicities.push(compact_int(data, &mut at)?);
    }
    if knots.iter().any(|v| !v.is_finite()) || !knots_strictly_increasing(&knots) {
        return None;
    }
    let blocks_end = at.checked_add(block_bytes.checked_mul(3)?)?;
    if multiplicities.first() != Some(&6)
        || multiplicities.last() != Some(&6)
        || multiplicities[1..multiplicities.len() - 1]
            .iter()
            .any(|value| !matches!(value, 1 | 3))
        || blocks_end > end
        || end - blocks_end != 59
    {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 10]>> {
        (0..count)
            .map(|site| {
                let mut values = [0.0; 10];
                for (channel, value) in values.iter_mut().enumerate() {
                    *value = f64_le(data, start + site * 80 + channel * 8)?;
                }
                values.iter().all(|v| v.is_finite()).then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A8FreeformCurve {
        pos,
        object_id,
        degree,
        knots,
        multiplicities,
        sites,
        first_derivatives,
        second_derivatives,
        tail_len: end - blocks_end,
    })
}

/// Decode framed `a5 03 32` rolling-ball jet records.
#[must_use]
#[cfg(test)]
pub fn a5_freeform_curves(data: &[u8]) -> Vec<A5FreeformCurve> {
    let records = consolidated_records(data);
    a5_freeform_curves_from_records(data, &records)
}

pub(crate) fn a5_freeform_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<A5FreeformCurve> {
    a_family_frames_from_records(records, 0x32)
        .into_iter()
        .filter_map(|frame| parse_a5_curve(data, frame))
        .collect()
}

fn parse_a5_curve(data: &[u8], frame: ConsolidatedFrame) -> Option<A5FreeformCurve> {
    let ConsolidatedFrame {
        pos,
        payload,
        end,
        header_token,
    } = frame;
    if data.get(pos) == Some(&0xa5) {
        let header_byte = u8::try_from(header_token).ok()?;
        a5_int(header_byte)?;
    }
    let mut at = payload;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    let degree = compact_int(data, &mut at)?;
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count || count < 2 || degree != 5 {
        return None;
    }
    match data.get(at..at + 2) {
        Some([0x0c, _]) => at += 1,
        Some([0x08, marker]) if a5_int(*marker).is_some() => at += 2,
        _ => return None,
    }
    let knot_bytes = count.checked_mul(8)?;
    let block_bytes = count.checked_mul(80)?;
    let known_bytes = knot_bytes.checked_add(block_bytes.checked_mul(3)?)?;
    if at.checked_add(known_bytes)? > end {
        return None;
    }
    let mut knots = Vec::with_capacity(count);
    for _ in 0..count {
        knots.push(f64_le(data, at)?);
        at += 8;
    }
    if knots.iter().any(|v| !v.is_finite()) || !knots_strictly_increasing(&knots) {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[f64; 10]>> {
        (0..count)
            .map(|site| {
                let mut values = [0.0; 10];
                for (channel, value) in values.iter_mut().enumerate() {
                    *value = f64_le(data, start + site * 80 + channel * 8)?;
                }
                values.iter().all(|v| v.is_finite()).then_some(values)
            })
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A5FreeformCurve {
        pos,
        header_token,
        degree,
        knots,
        sites,
        first_derivatives,
        second_derivatives,
    })
}

fn rolling_ball_sites(positions: Vec<[f64; 10]>) -> Option<Vec<RollingBallSite>> {
    let mut sites = Vec::with_capacity(positions.len());
    for v in positions {
        let limit1 = [v[0], v[1], v[2]];
        let limit2 = [v[3], v[4], v[5]];
        let center = [v[6], v[7], v[8]];
        let radius = distance3(center, limit1);
        let other = distance3(center, limit2);
        let chord = distance3(limit1, limit2);
        let radius_scale = radius.max(other);
        let relative_radius_difference = ((radius / radius_scale) - (other / radius_scale)).abs();
        if !radius.is_finite()
            || radius <= 0.0
            || !other.is_finite()
            || relative_radius_difference > EPS_ROLLING_BALL_RADIUS
            || (v[9] - 2.0 * ((chord / radius) * 0.5).clamp(-1.0, 1.0).asin()).abs()
                > EPS_ROLLING_BALL_ANGLE
        {
            return None;
        }
        sites.push(RollingBallSite {
            limit1,
            limit2,
            center,
            theta: v[9],
            radius,
        });
    }
    Some(sites)
}

fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}

/// Decode framed `a8 <flag> 20` UV jet records.
#[must_use]
#[cfg(test)]
pub fn a8_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    object_stream_pcurves(data)
        .into_iter()
        .filter(|pcurve| data.get(pcurve.pos) == Some(&0xa8))
        .collect()
}

/// Decode framed `a8 <flag> 20` and `b5 <flag> 20` object-stream UV jet records.
#[must_use]
pub fn object_stream_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    object_stream_frames(data)
        .into_iter()
        .filter(|frame| frame.class == 0x20)
        .filter_map(|frame| {
            parse_object_stream_pcurve(data, frame.pos, frame.payload, frame.end, frame.object_id)
        })
        .collect()
}

fn parse_object_stream_pcurve(
    data: &[u8],
    pos: usize,
    payload: usize,
    end: usize,
    object_id: u32,
) -> Option<A8Pcurve> {
    #[cfg(not(test))]
    let _ = pos;
    let mut at = payload + 1;
    let support_id = object_stream_reference(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    at += 2;
    data.get(..at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    at += if data.get(at) == Some(&0x08) { 2 } else { 1 };
    if count < 2 || degree != 5 {
        return None;
    }
    let knot_bytes = count.checked_mul(8)?;
    let array_bytes = count.checked_mul(8)?.checked_mul(6)?;
    let known_bytes = knot_bytes
        .checked_add(count)?
        .checked_add(array_bytes)?
        .checked_add(20)?;
    if at.checked_add(known_bytes)? > end {
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
    let mut multiplicities = Vec::with_capacity(count);
    for _ in 0..count {
        multiplicities.push(compact_int(data, &mut at)?);
    }
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    let mode = *data.get(at)?;
    at += 1;
    if at.checked_add(array_bytes.checked_add(18)?)? > end {
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
    if data.get(at) != Some(&0x07)
        || mode % 4 != 1
        || !knots_strictly_increasing(&knots)
        || multiplicities.first() != Some(&6)
        || multiplicities.last() != Some(&6)
        || multiplicities[1..multiplicities.len() - 1]
            .iter()
            .any(|multiplicity| *multiplicity != 3)
        || range[0] >= range[1]
        || end != at + 1
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
    Some(A8Pcurve {
        #[cfg(test)]
        pos,
        object_id,
        support_id,
        degree,
        knots,
        #[cfg(test)]
        mode,
        points: u.into_iter().zip(v).map(|p| [p.0, p.1]).collect(),
        first_derivatives: du.into_iter().zip(dv).map(|p| [p.0, p.1]).collect(),
        second_derivatives: ddu.into_iter().zip(ddv).map(|p| [p.0, p.1]).collect(),
        range,
    })
}

/// Decode common-form object-stream NURBS surfaces.  Every variable-length
/// field is bounded by the record's `payload_len`, so signature collisions do
/// not become carriers.
pub fn a8_surfaces(data: &[u8]) -> Vec<FreeformSurface> {
    a8_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| a8_surface_from_parsed(data, parse_a8_surface_header(data, frame)?))
        .collect()
}

/// Decode every complete common-form object-stream NURBS surface, including
/// parameter records whose pole grids occupy a uniquely bounded external
/// allocation.
#[must_use]
pub fn resolved_a8_surfaces(data: &[u8]) -> Vec<FreeformSurface> {
    a8_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| {
            resolved_a8_surface_from_object_frame(data, frame.pos, frame.end, frame.object_id)
        })
        .collect()
}

/// Decode every structurally complete `a8 <flag> 34` parameter lattice, including
/// records whose pole representation is not inline.
#[must_use]
pub fn a8_surface_headers(data: &[u8]) -> Vec<A8SurfaceHeader> {
    a8_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| {
            a8_surface_header_from_object_frame(data, frame.pos, frame.end, frame.object_id)
        })
        .collect()
}

/// Decode one selected `a8 <flag> 34` frame's parameter lattice.
pub(crate) fn a8_surface_header_from_object_frame(
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Option<A8SurfaceHeader> {
    parse_selected_a8_surface_header(data, start, end, object_id).map(|parsed| parsed.header)
}

/// Decode one selected `a8 <flag> 34` frame and its complete pole grid.
pub(crate) fn resolved_a8_surface_from_object_frame(
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Option<FreeformSurface> {
    let parsed = parse_selected_a8_surface_header(data, start, end, object_id)?;
    if parsed.header.pole_storage == PoleStorage::Elided {
        a8_surface_from_external_grid(data, &parsed.header)
    } else {
        a8_surface_from_parsed(data, parsed)
    }
}

/// Resolve an elided-pole `a8 <flag> 34` carrier from its support-referenced
/// external grid allocation. The allocation occupies the complete unframed gap
/// between a length-closed `b5 <flag> 21` pcurve and the following A/B-family
/// frame; its pcurve support reference must equal the surface object id.
#[must_use]
pub fn a8_surface_from_external_grid(
    data: &[u8],
    header: &A8SurfaceHeader,
) -> Option<FreeformSurface> {
    let candidates = a8_external_grid_candidates(data, header);
    let [ExternalGridCandidate {
        control_points,
        weights,
        ..
    }] = candidates.as_slice()
    else {
        return None;
    };
    Some(FreeformSurface {
        pos: header.pos,
        identity: FreeformSurfaceIdentity::Object(header.object_id),
        geometry: SurfaceGeometry::Nurbs(
            NurbsSurface::new(
                header.u_degree,
                header.v_degree,
                expand_knots(&header.u_distinct_knots, &header.u_multiplicities)?,
                expand_knots(&header.v_distinct_knots, &header.v_multiplicities)?,
                header.u_count,
                header.v_count,
                control_points.clone(),
                weights.clone(),
                false,
                false,
                false,
            )
            .ok()?,
        ),
    })
}

/// Return every complete support-bound external A8 pole allocation.
pub(crate) fn a8_external_grid_ranges(data: &[u8]) -> Vec<Range<usize>> {
    let mut ranges = a8_surface_headers(data)
        .into_iter()
        .flat_map(|header| {
            a8_external_grid_candidates(data, &header)
                .into_iter()
                .map(|candidate| candidate.range)
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    ranges.dedup();
    ranges
}

struct ExternalGridCandidate {
    range: Range<usize>,
    control_points: Vec<Point3>,
    weights: Option<Vec<f64>>,
}

fn a8_external_grid_candidates(
    data: &[u8],
    header: &A8SurfaceHeader,
) -> Vec<ExternalGridCandidate> {
    if header.pole_storage != PoleStorage::Elided {
        return Vec::new();
    }
    let (Ok(u_count), Ok(v_count)) = (
        usize::try_from(header.u_count),
        usize::try_from(header.v_count),
    ) else {
        return Vec::new();
    };
    let Some(poles) = crate::nurbs_surface_control_count(u_count, v_count) else {
        return Vec::new();
    };
    let weight_bytes = if header.rational {
        let Some(bytes) = poles.checked_mul(8) else {
            return Vec::new();
        };
        bytes
    } else {
        0
    };
    let Some(grid_bytes) = poles
        .checked_mul(24)
        .and_then(|bytes| bytes.checked_add(weight_bytes))
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for frame in object_stream_frames(data)
        .into_iter()
        .filter(|frame| frame.family == 0xb5 && frame.class == 0x21)
        .filter(|frame| {
            let Some(mut at) = frame.payload.checked_add(1) else {
                return false;
            };
            object_stream_reference(data, &mut at) == Some(header.object_id)
        })
    {
        let start = frame.end;
        let Some(end) = start.checked_add(grid_bytes) else {
            continue;
        };
        if object_stream_frame(data, end).is_none() {
            continue;
        }
        let mut at = start;
        let mut control_points = Vec::with_capacity(poles);
        let mut complete = true;
        for _ in 0..poles {
            let Some(point) = f64_point(data, at) else {
                complete = false;
                break;
            };
            control_points.push(point);
            at += 24;
        }
        if !complete
            || control_points
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .any(|coordinate| !coordinate.is_finite())
        {
            continue;
        }
        let weights = if header.rational {
            let Some(values) = f64_values(data, &mut at, poles, end) else {
                continue;
            };
            if values
                .iter()
                .any(|weight| !weight.is_finite() || *weight == 0.0)
            {
                continue;
            }
            Some(values)
        } else {
            None
        };
        if at == end {
            candidates.push(ExternalGridCandidate {
                range: start..end,
                control_points,
                weights,
            });
        }
    }
    candidates
}

/// Decode consolidated `a5 03 34` NURBS surface carriers.  This family uses
/// implicit clamped multiplicities instead of the explicit `a8` vectors.
pub fn a5_surfaces(data: &[u8]) -> Vec<FreeformSurface> {
    let records = consolidated_records(data);
    a5_surfaces_from_records(data, &records)
}

pub(crate) fn a5_surfaces_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<FreeformSurface> {
    a_family_frames_from_records(records, 0x34)
        .into_iter()
        .filter_map(|frame| a5_surface(data, frame))
        .collect()
}

fn a5_surface(data: &[u8], frame: ConsolidatedFrame) -> Option<FreeformSurface> {
    let ConsolidatedFrame {
        pos, payload, end, ..
    } = frame;
    let mut at = payload;
    let u_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let u_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?;
    let v_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let v_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?;
    let mode = *data.get(at)?;
    at += 1;
    if !strictly_increasing_finite(&u_distinct) || !strictly_increasing_finite(&v_distinct) {
        return None;
    }
    let (u_knots, u_count) = a5_knots(&u_distinct, u_degree)?;
    let (v_knots, v_count) = a5_knots(&v_distinct, v_degree)?;
    let poles = crate::nurbs_surface_control_count(u_count as usize, v_count as usize)?;
    if at.checked_add(poles.checked_mul(24)?)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, at)?);
        at += 24;
    }
    if control_points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .any(|coordinate| !coordinate.is_finite())
    {
        return None;
    }
    let weights = match mode {
        0x01 => None,
        0x05 => Some(a5_weights(
            data,
            &mut at,
            u_count as usize,
            v_count as usize,
            end,
        )?),
        _ => return None,
    };
    if !valid_a5_surface_tail(data, at, end) {
        return None;
    }
    Some(FreeformSurface {
        pos,
        identity: FreeformSurfaceIdentity::FrameOffset(pos),
        geometry: SurfaceGeometry::Nurbs(
            NurbsSurface::new(
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                u_count,
                v_count,
                control_points,
                weights,
                false,
                false,
                false,
            )
            .ok()?,
        ),
    })
}

struct ParsedA8SurfaceHeader {
    header: A8SurfaceHeader,
    pole_start: usize,
    end: usize,
}

fn parse_selected_a8_surface_header(
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Option<ParsedA8SurfaceHeader> {
    let frame = object_stream_frame(data, start)?;
    (frame.family == 0xa8
        && frame.class == 0x34
        && frame.end == end
        && frame.object_id == object_id)
        .then_some(())?;
    parse_a8_surface_header(
        data,
        A8Frame {
            pos: start,
            payload: frame.payload,
            end,
            object_id,
        },
    )
}

fn parse_a8_surface_header(data: &[u8], frame: A8Frame) -> Option<ParsedA8SurfaceHeader> {
    let A8Frame {
        pos,
        payload,
        end,
        object_id,
    } = frame;
    if end.checked_sub(payload)? < 20 {
        return None;
    }
    let mut at = payload.checked_add(1)?; // framing + lead byte
    let u_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?; // flags
    let u_distinct_count = compact_int(data, &mut at)? as usize;
    at = consume_array_marker(data, at)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?;
    let u_mults = compact_values(data, &mut at, u_distinct_count)?;
    let v_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    let v_distinct_count = compact_int(data, &mut at)? as usize;
    at = consume_array_marker(data, at)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?;
    let v_mults = compact_values(data, &mut at, v_distinct_count)?;
    let mode = *data.get(at)?;
    at += 1;
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || u_distinct_count < 2
        || v_distinct_count < 2
        || !matches!(mode, 0x01 | 0x05)
        || !strictly_increasing_finite(&u_distinct)
        || !strictly_increasing_finite(&v_distinct)
    {
        return None;
    }
    let u_count = pole_count(&u_mults, u_degree)?;
    let v_count = pole_count(&v_mults, v_degree)?;
    if u_count == 0 || v_count == 0 {
        return None;
    }
    let tail_end = at.checked_add(141)?;
    let elided = tail_end <= end
        && closed_a8_child_run(data, tail_end, end)
        && parse_a8_elided_surface_tail(data, at, &v_distinct).is_some();
    Some(ParsedA8SurfaceHeader {
        header: A8SurfaceHeader {
            pos,
            object_id,
            u_degree,
            v_degree,
            u_distinct_knots: u_distinct,
            v_distinct_knots: v_distinct,
            u_multiplicities: u_mults,
            v_multiplicities: v_mults,
            u_count,
            v_count,
            rational: mode == 0x05,
            pole_storage: if elided {
                PoleStorage::Elided
            } else {
                PoleStorage::Inline
            },
        },
        pole_start: at,
        end,
    })
}

fn a8_surface_from_parsed(data: &[u8], parsed: ParsedA8SurfaceHeader) -> Option<FreeformSurface> {
    let ParsedA8SurfaceHeader {
        header,
        mut pole_start,
        end,
    } = parsed;
    let A8SurfaceHeader {
        pos,
        object_id,
        u_degree,
        v_degree,
        u_distinct_knots,
        v_distinct_knots,
        u_multiplicities,
        v_multiplicities,
        u_count,
        v_count,
        rational,
        pole_storage,
        ..
    } = header;
    if pole_storage == PoleStorage::Elided {
        return None;
    }
    let poles = crate::nurbs_surface_control_count(u_count as usize, v_count as usize)?;
    let pole_bytes = poles.checked_mul(24)?;
    if pole_start.checked_add(pole_bytes)? > end {
        return None;
    }
    let mut control_points = Vec::with_capacity(poles);
    for _ in 0..poles {
        control_points.push(f64_point(data, pole_start)?);
        pole_start += 24;
    }
    if control_points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .any(|coordinate| !coordinate.is_finite())
    {
        return None;
    }
    let weights = if rational {
        let values = f64_values(data, &mut pole_start, poles, end)?;
        values
            .iter()
            .all(|weight| weight.is_finite() && *weight != 0.0)
            .then_some(values)?
    } else {
        Vec::new()
    };
    a8_surface_suffix_start(data, pole_start, end)?;
    Some(FreeformSurface {
        pos,
        identity: FreeformSurfaceIdentity::Object(object_id),
        geometry: SurfaceGeometry::Nurbs(
            NurbsSurface::new(
                u_degree,
                v_degree,
                expand_knots(&u_distinct_knots, &u_multiplicities)?,
                expand_knots(&v_distinct_knots, &v_multiplicities)?,
                u_count,
                v_count,
                control_points,
                rational.then_some(weights),
                false,
                false,
                false,
            )
            .ok()?,
        ),
    })
}

fn object_stream_reference(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let lead = *bytes.get(*at)?;
    let (value, width) = match lead {
        0x38 => (u32_le_24(bytes, *at + 1)?, 4),
        0x30 => (u32::from(View::u16_le_at(bytes, *at + 1)?) << 8, 3),
        0x28 => (
            u32::from(*bytes.get(*at + 1)?) | (u32::from(*bytes.get(*at + 2)?) << 16),
            3,
        ),
        0x20 => (u32::from(*bytes.get(*at + 1)?) << 16, 2),
        0x18 => (u32::from(View::u16_le_at(bytes, *at + 1)?), 3),
        0x10 => (u32::from(*bytes.get(*at + 1)?) << 8, 2),
        0x08 => (u32::from(*bytes.get(*at + 1)?), 2),
        0x80..=0xff => (u32::from(lead - 0x80), 1),
        _ => return None,
    };
    *at += width;
    Some(value)
}

fn consume_array_marker(bytes: &[u8], at: usize) -> Option<usize> {
    if *bytes.get(at)? == 0x08 {
        bytes.get(at + 1).map(|_| at + 2)
    } else {
        Some(at + 1)
    }
}

fn f64_values(bytes: &[u8], at: &mut usize, count: usize, end: usize) -> Option<Vec<f64>> {
    if at.checked_add(count.checked_mul(8)?)? > end {
        return None;
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(f64_le(bytes, *at)?);
        *at += 8;
    }
    Some(values)
}

fn compact_values(bytes: &[u8], at: &mut usize, count: usize) -> Option<Vec<u32>> {
    (0..count).map(|_| compact_int(bytes, at)).collect()
}

fn strictly_increasing_finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite()) && knots_strictly_increasing(values)
}

fn a5_int(byte: u8) -> Option<u32> {
    (byte % 4 == 1).then(|| u32::from((byte - 1) / 4))
}

fn a5_array_marker(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at..at + 2) {
        Some([0x0c, ..]) => Some(at + 1),
        Some([0x08, 0x09]) => Some(at + 2),
        _ => None,
    }
}

pub(super) fn a5_knots(distinct: &[f64], degree: u32) -> Option<(Vec<f64>, u32)> {
    let multiplicities = match degree {
        1 | 3 if distinct.len() >= 2 => {
            let mut values = vec![degree + 1];
            values.extend(std::iter::repeat_n(1, distinct.len() - 2));
            values.push(degree + 1);
            values
        }
        5 if distinct.len() >= 2 => {
            let mut values = vec![6u32];
            values.extend(std::iter::repeat_n(3, distinct.len() - 2));
            values.push(6);
            values
        }
        _ => return None,
    };
    let count = pole_count(&multiplicities, degree)?;
    Some((expand_knots(distinct, &multiplicities)?, count))
}

pub(super) fn a5_weights(
    bytes: &[u8],
    at: &mut usize,
    rows: usize,
    cols: usize,
    end: usize,
) -> Option<Vec<f64>> {
    let count = rows.checked_mul(cols)?;
    if bytes.get(*at) == Some(&0x00) {
        *at += 1;
        return f64_values(bytes, at, count, end).filter(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight != 0.0)
        });
    }
    if bytes.get(*at) != Some(&0x01) {
        return None;
    }

    let seed_count = cols.div_ceil(2);
    let mut weights = Vec::with_capacity(count);
    let mut previous = None::<Vec<f64>>;
    for _ in 0..rows {
        let row = if bytes.get(*at) == Some(&0x02) {
            *at += 1;
            previous.clone()?
        } else {
            if !matches!(bytes.get(*at..*at + 3), Some([0x01, 0x03 | 0x07, 0x00])) {
                return None;
            }
            *at += 3;
            let seed = f64_values(bytes, at, seed_count, end)?;
            let mut row = seed.clone();
            row.extend(seed[..cols / 2].iter().rev().copied());
            if row.len() != cols {
                return None;
            }
            previous = Some(row.clone());
            row
        };
        weights.extend(row);
    }
    weights
        .iter()
        .all(|weight| weight.is_finite() && *weight != 0.0)
        .then_some(weights)
}
