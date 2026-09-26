//! A-family consolidated curve and surface record vocabulary.
//!
//! Decodes `a5`/`a8` NURBS surface carriers, common-form and consolidated
//! rolling-ball jets, guide-curve jets, and object-stream UV pcurves.

use super::knot_lane::A8KnotLane;
use crate::math::distance;
use crate::nurbs::{expand_knots, pole_count};
use crate::wire::bytes::{
    compact_int, f64_le, f64_point, finite_f64_lane, read_f64_array, u32_le_24,
};
#[cfg(test)]
use crate::wire::records::ConsolidatedPcurve;
use crate::wire::records::{
    consolidated_records, family_frames_from_records, ConsolidatedFamily, ConsolidatedFrame,
    ConsolidatedRecord,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{
    nurbs::{knots_strictly_increasing, NurbsCurve, NurbsSurface},
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::units::FiniteVector;
use std::ops::Range;

const EPS_GUIDE_DIRECTION_UNIT: f64 = 1.0e-9;
const EPS_ROLLING_BALL_RADIUS: f64 = 1.0e-9;
const EPS_ROLLING_BALL_ANGLE: f64 = 1.0e-9;

/// A decoded common-form or consolidated freeform NURBS surface.
#[derive(Debug, Clone)]
pub(crate) struct FreeformSurface {
    /// Source offset of the framed record.
    pub(in crate::families) pos: usize,
    /// Inline persistent object id for an A8 carrier. `None` for an A5
    /// carrier identified only by [`Self::pos`].
    pub(in crate::families) identity: Option<u32>,
    /// The decoded NURBS carrier.
    pub(in crate::families) geometry: NurbsSurface,
}

/// Whether an `a8 <flag> 34` surface stores poles inline or in an external grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PoleStorage {
    /// Pole and weight grid occupy the payload after the mode byte.
    Inline,
    /// The fixed 141-byte surface tail begins immediately after the mode byte.
    Elided,
}

impl FreeformSurface {
    /// Return the inline persistent object id when this is an A8 carrier.
    #[must_use]
    pub(in crate::families) fn object_id(&self) -> Option<u32> {
        self.identity
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
pub(in crate::families) fn a8_nested_b5_run_start(
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
            usize::try_from(parsed.header.u_count()?).ok()?,
            usize::try_from(parsed.header.v_count()?).ok()?,
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

fn parse_a8_elided_surface_tail(data: &[u8], at: usize, v_knots: &[FiniteReal]) -> Option<usize> {
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
    let expected_v_span = v_last.get() - v_first.get();
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
    let parameters = read_f64_array::<8>(tail, 4)?.map(FiniteReal::get);
    if parameters[0] >= parameters[1]
        || parameters[2] >= parameters[3]
        || parameters[4] == 0.0
        || parameters[6] == 0.0
    {
        return None;
    }
    let continuation_start = 71;
    let continuation_end = continuation_start + continuation_bytes;
    let continuation = finite_f64_lane(tail.get(continuation_start..continuation_end)?)?;
    let suffix = &tail[continuation_end..];
    let valid_suffix = match (tail_len, &tail[68..71]) {
        (133, [0x01, 0x01, 0x01]) => {
            continuation.iter().all(|value| value.get() == 0.0)
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
pub(in crate::families) struct A8SurfaceHeader {
    /// Source offset of the framed record.
    pos: usize,
    /// Inline persistent object id.
    pub(in crate::families) object_id: u32,
    /// U degree.
    pub(super) u_degree: u32,
    /// V degree.
    pub(super) v_degree: u32,
    /// U knots and multiplicities.
    pub(super) u_knots: A8KnotLane,
    /// V knots and multiplicities.
    pub(super) v_knots: A8KnotLane,
    /// Whether the record selects rational weights.
    rational: bool,
    /// Whether poles occupy the payload or an external grid.
    pole_storage: PoleStorage,
}

impl A8SurfaceHeader {
    /// U pole count derived from degree and knot multiplicities.
    fn u_count(&self) -> Option<u32> {
        self.u_knots.pole_count(self.u_degree)
    }

    /// V pole count derived from degree and knot multiplicities.
    fn v_count(&self) -> Option<u32> {
        self.v_knots.pole_count(self.v_degree)
    }
}

#[derive(Debug, Clone)]
/// Degree-5 UV jet stored in an `a8 <flag> 20` object record.
pub(in crate::families) struct A8Pcurve {
    /// Inline object identifier.
    pub(in crate::families) object_id: u32,
    /// Referenced support-surface object identifier.
    pub(in crate::families) support_id: u32,
    /// Stored UV-jet channel-mode byte.
    #[cfg(test)]
    pub(super) mode: u8,
    /// Knot-aligned UV jet sites.
    pub(in crate::families) sites: Vec<A8PcurveSite>,
    /// Native parameter range.
    pub(in crate::families) range: [FiniteReal; 2],
}

/// One knot and its complete UV jet.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct A8PcurveSite {
    knot: FiniteReal,
    point: FiniteVector<2>,
    first_derivative: FiniteVector<2>,
    second_derivative: FiniteVector<2>,
}

impl A8Pcurve {
    pub(in crate::families) const DEGREE: u32 = 5;

    pub(in crate::families) fn knots(&self) -> Vec<FiniteReal> {
        self.sites.iter().map(|site| site.knot).collect()
    }

    #[cfg(test)]
    fn points(&self) -> Vec<[f64; 2]> {
        self.sites.iter().map(|site| site.point.get()).collect()
    }

    pub(in crate::families) fn bspline(&self) -> Option<(Vec<f64>, Vec<FiniteVector<2>>)> {
        crate::nurbs::quintic_jet_bspline(
            Self::DEGREE,
            &self
                .sites
                .iter()
                .map(|site| site.knot.get())
                .collect::<Vec<_>>(),
            &self
                .sites
                .iter()
                .map(|site| site.point.get())
                .collect::<Vec<_>>(),
            &self
                .sites
                .iter()
                .map(|site| site.first_derivative.get())
                .collect::<Vec<_>>(),
            &self
                .sites
                .iter()
                .map(|site| site.second_derivative.get())
                .collect::<Vec<_>>(),
        )
    }
}

/// Decode framed `a5 03 20` consolidated UV jets.
#[must_use]
#[cfg(test)]
fn a5_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    let records = consolidated_records(data);
    crate::wire::records::family_pcurves_from_records(data, &records, ConsolidatedFamily::A)
}

/// One knot-site value in an `a5 03 32` rolling-ball program.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct RollingBallSite {
    /// First limiting curve point.
    pub(in crate::families) limit1: FinitePoint3,
    /// Second limiting curve point.
    pub(in crate::families) limit2: FinitePoint3,
    /// Rolling-ball centre.
    pub(in crate::families) center: FinitePoint3,
    /// Stored opening angle.
    pub(in crate::families) theta: FiniteReal,
}

#[cfg(test)]
impl RollingBallSite {
    /// Radius from the centre to the first limit.
    fn radius(&self) -> f64 {
        distance(self.center.get(), self.limit1.get())
    }
}

/// One knot of a degree-5 rolling-ball jet.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct A5FreeformJet {
    /// Distinct knot.
    pub(in crate::families) knot: FiniteReal,
    /// Position channels at this knot.
    pub(in crate::families) site: RollingBallSite,
    /// Ten first-derivative channels.
    pub(in crate::families) first_derivatives: [FiniteReal; 10],
    /// Ten second-derivative channels.
    pub(in crate::families) second_derivatives: [FiniteReal; 10],
}

/// Consolidated degree-5 rolling-ball jet.
#[derive(Debug, Clone)]
pub(in crate::families) struct A5FreeformCurve {
    /// Record byte offset.
    pub(in crate::families) pos: usize,
    /// Schema token immediately before the payload.
    pub(in crate::families) header_token: u32,
    /// Knot-aligned jet samples.
    pub(in crate::families) sites: Vec<A5FreeformJet>,
}

impl A5FreeformCurve {
    pub(in crate::families) const DEGREE: u32 = 5;

    fn knots(&self) -> Vec<f64> {
        self.sites.iter().map(|site| site.knot.get()).collect()
    }
}

/// Lower either limiting locus of a complete rolling-ball jet to its exact
/// degree-5 NURBS representation.
pub(in crate::families) fn rolling_ball_limit_curve(
    jet: &A5FreeformCurve,
    second_limit: bool,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsCurve> {
    let offset = usize::from(second_limit) * 3;
    let positions = jet
        .sites
        .iter()
        .map(|sample| {
            let limit = if second_limit {
                sample.site.limit2
            } else {
                sample.site.limit1
            };
            limit.get().into()
        })
        .collect::<Vec<_>>();
    let first = jet
        .sites
        .iter()
        .map(|sample| {
            let values = FiniteReal::raw_array(sample.first_derivatives);
            [values[offset], values[offset + 1], values[offset + 2]]
        })
        .collect::<Vec<_>>();
    let second = jet
        .sites
        .iter()
        .map(|sample| {
            let values = FiniteReal::raw_array(sample.second_derivatives);
            [values[offset], values[offset + 1], values[offset + 2]]
        })
        .collect::<Vec<_>>();
    let knots = jet.knots();
    let Some((knots, control_points)) = crate::nurbs::quintic_jet_bspline(
        A5FreeformCurve::DEGREE,
        &knots,
        &positions,
        &first,
        &second,
    ) else {
        refusal.push_solver(
            format_args!(
                "consolidated_a5_03_32 rolling-ball limit curve at byte {}",
                jet.pos
            ),
            "states knot-aligned jet samples the degree-5 B-spline lowering does not close",
        );
        return None;
    };
    crate::nurbs::note_refusal(
        NurbsCurve::from_lanes(
            A5FreeformCurve::DEGREE,
            knots,
            control_points
                .into_iter()
                .map(|point| Point3::new(point[0], point[1], point[2]))
                .collect(),
            None,
            false,
        ),
        refusal,
        format_args!(
            "consolidated_a5_03_32 rolling-ball limit curve at byte {}",
            jet.pos
        ),
    )
}

/// One position in an `a5/a6/a7 03 39` jet.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct GuideCurveSite {
    /// Parameter knot.
    knot: FiniteReal,
    /// Six first-derivative channels.
    pub(in crate::families) first_derivative: FiniteVector<6>,
    /// Six second-derivative channels.
    pub(in crate::families) second_derivative: FiniteVector<6>,
    /// Guide-curve point.
    pub(in crate::families) point: FiniteVector<3>,
}

/// Width-coded guide-curve and reference-direction jet.
#[derive(Debug, Clone)]
pub(in crate::families) struct A5GuideCurve {
    /// Record byte offset.
    pub(in crate::families) pos: usize,
    /// Width-coded header token.
    pub(in crate::families) header_token: u32,
    /// Parametric degree.
    pub(in crate::families) degree: u32,
    /// Positions whose source triples pass the unit-direction gate.
    pub(in crate::families) sites: Vec<GuideCurveSite>,
}

impl A5GuideCurve {
    pub(in crate::families) fn knots(&self) -> Vec<f64> {
        self.sites.iter().map(|site| site.knot.get()).collect()
    }
}

/// One non-rational degree-5 NURBS curve stored in an `a5 13 16` frame.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct A5NurbsCurve {
    /// Record byte offset.
    pub(in crate::families) pos: usize,
    /// Width-coded record token.
    pub(in crate::families) header_token: u32,
    /// Exact neutral curve.
    pub(in crate::families) geometry: NurbsCurve,
}

/// Decode length-closed `a5/a6/a7 13 16` non-rational NURBS curves.
#[must_use]
#[cfg(test)]
fn a5_nurbs_curves(data: &[u8]) -> Vec<A5NurbsCurve> {
    let records = consolidated_records(data);
    a5_nurbs_curves_from_records(data, &records, &mut crate::nurbs::LaneRefusals::new())
}

pub(in crate::families) fn a5_nurbs_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<A5NurbsCurve> {
    family_frames_from_records(records, ConsolidatedFamily::A, 0x16)
        .into_iter()
        .filter_map(|frame| parse_a5_nurbs_curve(data, frame, refusal))
        .collect()
}

fn parse_a5_nurbs_curve(
    data: &[u8],
    frame: ConsolidatedFrame,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<A5NurbsCurve> {
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
        distinct_knots.push(f64_le(data, at)?.get());
        at += 8;
    }
    if !knots_strictly_increasing(&distinct_knots) || data.get(at) != Some(&0x01) {
        return None;
    }
    at += 1;
    let control_points = (0..control_count)
        .map(|_| {
            let point = f64_point(data, at)?;
            at += 24;
            Some(point)
        })
        .collect::<Option<Vec<_>>>()?;
    if compact_int(data, &mut at)? != 1 || compact_int(data, &mut at)? != 2 {
        return None;
    }
    let range_origin = f64_le(data, at)?.get();
    let repeated_end = f64_le(data, at + 8)?.get();
    let scale = f64_le(data, at + 16)?.get();
    let offset = f64_le(data, at + 24)?.get();
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
        geometry: crate::nurbs::note_refusal(
            NurbsCurve::from_lanes(degree, knots, control_points, None, false),
            refusal,
            format_args!("a5 NURBS curve record at byte {}", frame.pos),
        )?,
    })
}

/// Decode `a5/a6/a7 03 39` guide-curve and unit-direction jets.
#[must_use]
#[cfg(test)]
fn a5_guide_curves(data: &[u8]) -> Vec<A5GuideCurve> {
    let records = consolidated_records(data);
    a5_guide_curves_from_records(data, &records)
}

pub(in crate::families) fn a5_guide_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<A5GuideCurve> {
    family_frames_from_records(records, ConsolidatedFamily::A, 0x39)
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
    if !knots_strictly_increasing(&FiniteReal::raw_lane(&knots)) {
        return None;
    }
    if at
        .checked_add(block_bytes.checked_mul(3)?)?
        .checked_add(48)?
        != frame.end
    {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[FiniteReal; 6]>> {
        (0..count)
            .map(|site| read_f64_array::<6>(data, start + site * 48))
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites: Option<Vec<_>> = positions
        .into_iter()
        .zip(knots)
        .zip(first_derivatives)
        .zip(second_derivatives)
        .map(|(((value, knot), first_derivative), second_derivative)| {
            let direction = [
                value[3].get() - value[0].get(),
                value[4].get() - value[1].get(),
                value[5].get() - value[2].get(),
            ];
            let length =
                (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
            ((length - 1.0).abs() < EPS_GUIDE_DIRECTION_UNIT).then_some(GuideCurveSite {
                knot,
                first_derivative: first_derivative.into(),
                second_derivative: second_derivative.into(),
                point: [value[0], value[1], value[2]].into(),
            })
        })
        .collect();
    Some(A5GuideCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        degree,
        sites: sites?,
    })
}

/// One knot of a common-form degree-5 rolling-ball jet.
#[derive(Debug, Clone, PartialEq)]
struct A8FreeformJet {
    /// Distinct knot.
    knot: FiniteReal,
    /// Multiplicity of this distinct knot.
    multiplicity: u32,
    /// Position channels at this knot.
    pub(super) site: RollingBallSite,
    /// Ten first-derivative channels.
    first_derivatives: [FiniteReal; 10],
    /// Ten second-derivative channels.
    second_derivatives: [FiniteReal; 10],
}

/// Common-form degree-5 rolling-ball jet stored in an `a8 <flag> 32` object record.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct A8FreeformCurve {
    /// Record byte offset.
    pub(in crate::families) pos: usize,
    /// Inline persistent object identifier.
    pub(in crate::families) object_id: u32,
    /// Knot-aligned jet samples.
    sites: Vec<A8FreeformJet>,
}

impl A8FreeformCurve {
    const DEGREE: u32 = 5;

    pub(in crate::families) fn multiplicities(&self) -> Vec<u32> {
        self.sites.iter().map(|site| site.multiplicity).collect()
    }
}

/// Convert a complete common-form rolling-ball jet to its exact neutral
/// procedural carrier.
pub(in crate::families) fn rolling_ball_jet_definition(
    jet: &A8FreeformCurve,
) -> Option<ProceduralSurfaceDefinition> {
    if jet.sites.is_empty() {
        return None;
    }
    let stations = jet
        .sites
        .iter()
        .map(|sample| cadmpeg_ir::geometry::RollingBallJetStation {
            knot: sample.knot,
            multiplicity: sample.multiplicity,
            site: rolling_ball_jet_site(
                &sample.site,
                sample.first_derivatives,
                sample.second_derivatives,
            ),
        })
        .collect();
    Some(ProceduralSurfaceDefinition::RollingBallJet(
        cadmpeg_ir::geometry::RollingBallJetStations::from_admitted(
            A8FreeformCurve::DEGREE,
            stations,
        )
        .ok()?,
    ))
}

/// The admitted neutral jet site of one decoded rolling-ball site and its two
/// ten-channel derivative rows.
pub(in crate::families) fn rolling_ball_jet_site(
    site: &RollingBallSite,
    first_derivatives: [FiniteReal; 10],
    second_derivatives: [FiniteReal; 10],
) -> RollingBallJetSite<FiniteReal, FiniteVector3, FinitePoint3> {
    RollingBallJetSite {
        first_limit: site.limit1,
        second_limit: site.limit2,
        center: site.center,
        angle: site.theta,
        first_derivative: rolling_ball_jet_derivative(first_derivatives),
        second_derivative: rolling_ball_jet_derivative(second_derivatives),
    }
}

pub(in crate::families) fn rolling_ball_jet_derivative(
    values: [FiniteReal; 10],
) -> RollingBallJetDerivative<FiniteReal, FiniteVector3> {
    RollingBallJetDerivative {
        first_limit: FiniteVector3::from_components(values[0], values[1], values[2]),
        second_limit: FiniteVector3::from_components(values[3], values[4], values[5]),
        center: FiniteVector3::from_components(values[6], values[7], values[8]),
        angle: values[9],
    }
}

/// Decode framed `a8 <flag> 32` common-form rolling-ball jet records.
#[must_use]
pub(in crate::families) fn a8_freeform_curves(data: &[u8]) -> Vec<A8FreeformCurve> {
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
    if !knots_strictly_increasing(&FiniteReal::raw_lane(&knots)) {
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
    let block = |start: usize| -> Option<Vec<[FiniteReal; 10]>> {
        (0..count)
            .map(|site| read_f64_array::<10>(data, start + site * 80))
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A8FreeformCurve {
        pos,
        object_id,
        sites: knots
            .into_iter()
            .zip(multiplicities)
            .zip(sites)
            .zip(first_derivatives)
            .zip(second_derivatives)
            .map(
                |((((knot, multiplicity), site), first_derivatives), second_derivatives)| {
                    A8FreeformJet {
                        knot,
                        multiplicity,
                        site,
                        first_derivatives,
                        second_derivatives,
                    }
                },
            )
            .collect(),
    })
}

/// Decode framed `a5 03 32` rolling-ball jet records.
#[must_use]
#[cfg(test)]
fn a5_freeform_curves(data: &[u8]) -> Vec<A5FreeformCurve> {
    let records = consolidated_records(data);
    a5_freeform_curves_from_records(data, &records)
}

pub(in crate::families) fn a5_freeform_curves_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<A5FreeformCurve> {
    family_frames_from_records(records, ConsolidatedFamily::A, 0x32)
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
    if !knots_strictly_increasing(&FiniteReal::raw_lane(&knots)) {
        return None;
    }
    let block = |start: usize| -> Option<Vec<[FiniteReal; 10]>> {
        (0..count)
            .map(|site| read_f64_array::<10>(data, start + site * 80))
            .collect()
    };
    let positions = block(at)?;
    let first_derivatives = block(at + block_bytes)?;
    let second_derivatives = block(at + 2 * block_bytes)?;
    let sites = rolling_ball_sites(positions)?;
    Some(A5FreeformCurve {
        pos,
        header_token,
        sites: knots
            .into_iter()
            .zip(sites)
            .zip(first_derivatives)
            .zip(second_derivatives)
            .map(
                |(((knot, site), first_derivatives), second_derivatives)| A5FreeformJet {
                    knot,
                    site,
                    first_derivatives,
                    second_derivatives,
                },
            )
            .collect(),
    })
}

fn rolling_ball_sites(positions: Vec<[FiniteReal; 10]>) -> Option<Vec<RollingBallSite>> {
    let mut sites = Vec::with_capacity(positions.len());
    for values in positions {
        let limit1 = FinitePoint3::from_coordinates(values[0], values[1], values[2]);
        let limit2 = FinitePoint3::from_coordinates(values[3], values[4], values[5]);
        let center = FinitePoint3::from_coordinates(values[6], values[7], values[8]);
        let theta = values[9];
        let radius = distance(center.get(), limit1.get());
        let other = distance(center.get(), limit2.get());
        let chord = distance(limit1.get(), limit2.get());
        let radius_scale = radius.max(other);
        let relative_radius_difference = ((radius / radius_scale) - (other / radius_scale)).abs();
        if !radius.is_finite()
            || radius <= 0.0
            || !other.is_finite()
            || relative_radius_difference > EPS_ROLLING_BALL_RADIUS
            || (theta.get() - 2.0 * ((chord / radius) * 0.5).clamp(-1.0, 1.0).asin()).abs()
                > EPS_ROLLING_BALL_ANGLE
        {
            return None;
        }
        sites.push(RollingBallSite {
            limit1,
            limit2,
            center,
            theta,
        });
    }
    Some(sites)
}

/// Decode framed `a8 <flag> 20` UV jet records.
#[must_use]
#[cfg(test)]
fn a8_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    object_stream_frames(data)
        .into_iter()
        .filter(|frame| frame.class == 0x20 && data.get(frame.pos) == Some(&0xa8))
        .filter_map(|frame| {
            parse_object_stream_pcurve(data, frame.payload, frame.end, frame.object_id)
        })
        .collect()
}

/// Decode framed `a8 <flag> 20` and `b5 <flag> 20` object-stream UV jet records.
#[must_use]
pub(in crate::families) fn object_stream_pcurves(data: &[u8]) -> Vec<A8Pcurve> {
    object_stream_frames(data)
        .into_iter()
        .filter(|frame| frame.class == 0x20)
        .filter_map(|frame| {
            parse_object_stream_pcurve(data, frame.payload, frame.end, frame.object_id)
        })
        .collect()
}

fn parse_object_stream_pcurve(
    data: &[u8],
    payload: usize,
    end: usize,
    object_id: u32,
) -> Option<A8Pcurve> {
    let mut at = payload + 1;
    let support_id = object_stream_reference(data, &mut at)?;
    let degree = compact_int(data, &mut at)?;
    at += 2;
    data.get(..at)?;
    let count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    at += if data.get(at) == Some(&0x08) { 2 } else { 1 };
    if count < 2 || degree != A8Pcurve::DEGREE {
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
    let read_finite = |at: &mut usize| -> Option<Vec<FiniteReal>> {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(f64_le(data, *at)?);
            *at += 8;
        }
        Some(values)
    };
    let knots = read_finite(&mut at)?;
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
    let u = read_finite(&mut at)?;
    let v = read_finite(&mut at)?;
    let du = read_finite(&mut at)?;
    let dv = read_finite(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = read_finite(&mut at)?;
    let ddv = read_finite(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if data.get(at) != Some(&0x07)
        || mode % 4 != 1
        || !knots.windows(2).all(|pair| pair[0] < pair[1])
        || multiplicities.first() != Some(&6)
        || multiplicities.last() != Some(&6)
        || multiplicities[1..multiplicities.len() - 1]
            .iter()
            .any(|multiplicity| *multiplicity != 3)
        || range[0] >= range[1]
        || end != at + 1
    {
        return None;
    }
    Some(A8Pcurve {
        object_id,
        support_id,
        #[cfg(test)]
        mode,
        sites: knots
            .into_iter()
            .zip(u.into_iter().zip(v))
            .zip(du.into_iter().zip(dv))
            .zip(ddu.into_iter().zip(ddv))
            .map(|(((knot, (u, v)), (du, dv)), (ddu, ddv))| A8PcurveSite {
                knot,
                point: [u, v].into(),
                first_derivative: [du, dv].into(),
                second_derivative: [ddu, ddv].into(),
            })
            .collect(),
        range,
    })
}

/// Decode common-form object-stream NURBS surfaces.  Every variable-length
/// field is bounded by the record's `payload_len`, so signature collisions do
/// not become carriers.
pub(crate) fn a8_surfaces(
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<FreeformSurface> {
    a8_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| {
            a8_surface_from_parsed(data, parse_a8_surface_header(data, frame)?, refusal)
        })
        .collect()
}

/// Decode every complete common-form object-stream NURBS surface, including
/// parameter records whose pole grids occupy a uniquely bounded external
/// allocation.
#[must_use]
pub(in crate::families) fn resolved_a8_surfaces(
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<FreeformSurface> {
    a8_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| {
            resolved_a8_surface_from_object_frame(
                data,
                frame.pos,
                frame.end,
                frame.object_id,
                refusal,
            )
        })
        .collect()
}

/// Decode every structurally complete `a8 <flag> 34` parameter lattice, including
/// records whose pole representation is not inline.
#[must_use]
fn a8_surface_headers(data: &[u8]) -> Vec<A8SurfaceHeader> {
    a8_frames(data, 0x34)
        .into_iter()
        .filter_map(|frame| {
            a8_surface_header_from_object_frame(data, frame.pos, frame.end, frame.object_id)
        })
        .collect()
}

/// Decode one selected `a8 <flag> 34` frame's parameter lattice.
pub(in crate::families) fn a8_surface_header_from_object_frame(
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Option<A8SurfaceHeader> {
    parse_selected_a8_surface_header(data, start, end, object_id).map(|parsed| parsed.header)
}

/// Decode one selected `a8 <flag> 34` frame and its complete pole grid.
pub(in crate::families) fn resolved_a8_surface_from_object_frame(
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<FreeformSurface> {
    let parsed = parse_selected_a8_surface_header(data, start, end, object_id)?;
    if parsed.header.pole_storage == PoleStorage::Elided {
        a8_surface_from_external_grid(data, &parsed.header, refusal)
    } else {
        a8_surface_from_parsed(data, parsed, refusal)
    }
}

/// Resolve an elided-pole `a8 <flag> 34` carrier from its support-referenced
/// external grid allocation. The allocation occupies the complete unframed gap
/// between a length-closed `b5 <flag> 21` pcurve and the following A/B-family
/// frame; its pcurve support reference must equal the surface object id.
#[must_use]
fn a8_surface_from_external_grid(
    data: &[u8],
    header: &A8SurfaceHeader,
    refusal: &mut crate::nurbs::LaneRefusals,
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
    let row_len = header.v_count()? as usize;
    Some(FreeformSurface {
        pos: header.pos,
        identity: Some(header.object_id),
        geometry: crate::nurbs::note_refusal(
            NurbsSurface::from_checked_lanes(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    header.u_degree,
                    header.u_knots.expanded()?,
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    header.v_degree,
                    header.v_knots.expanded()?,
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    control_points
                        .clone()
                        .chunks(row_len)
                        .map(<[_]>::to_vec)
                        .collect(),
                    weights
                        .clone()
                        .map(|values| values.chunks(row_len).map(<[_]>::to_vec).collect()),
                ),
                false,
            ),
            refusal,
            format_args!(
                "a8 NURBS surface record #{} at byte {}",
                header.object_id, header.pos
            ),
        )?,
    })
}

/// Return every complete support-bound external A8 pole allocation.
pub(in crate::families) fn a8_external_grid_ranges(data: &[u8]) -> Vec<Range<usize>> {
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
    control_points: Vec<FinitePoint3>,
    weights: Option<Vec<NonZeroReal>>,
}

fn a8_external_grid_candidates(
    data: &[u8],
    header: &A8SurfaceHeader,
) -> Vec<ExternalGridCandidate> {
    if header.pole_storage != PoleStorage::Elided {
        return Vec::new();
    }
    let (Some(u_count), Some(v_count)) = (
        header
            .u_count()
            .and_then(|count| usize::try_from(count).ok()),
        header
            .v_count()
            .and_then(|count| usize::try_from(count).ok()),
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
        if !complete {
            continue;
        }
        let weights = if header.rational {
            let Some(values) = f64_values(data, &mut at, poles, end) else {
                continue;
            };
            let Some(values) = values
                .into_iter()
                .map(|value| NonZeroReal::new(value.get()))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
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
pub(crate) fn a5_surfaces(
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<FreeformSurface> {
    let records = consolidated_records(data);
    a5_surfaces_from_records(data, &records, refusal)
}

pub(in crate::families) fn a5_surfaces_from_records(
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<FreeformSurface> {
    family_frames_from_records(records, ConsolidatedFamily::A, 0x34)
        .into_iter()
        .filter_map(|frame| a5_surface(data, frame, refusal))
        .collect()
}

fn a5_surface(
    data: &[u8],
    frame: ConsolidatedFrame,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<FreeformSurface> {
    let ConsolidatedFrame {
        pos, payload, end, ..
    } = frame;
    let mut at = payload;
    let u_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let u_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let u_distinct = f64_values(data, &mut at, u_distinct_count, end)?
        .into_iter()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    let v_degree = a5_int(*data.get(at)?)?;
    at += 1;
    let v_distinct_count = a5_int(*data.get(at)?)? as usize;
    at = a5_array_marker(data, at + 1)?;
    let v_distinct = f64_values(data, &mut at, v_distinct_count, end)?
        .into_iter()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    let mode = *data.get(at)?;
    at += 1;
    if !knots_strictly_increasing(&u_distinct) || !knots_strictly_increasing(&v_distinct) {
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
        identity: None,
        geometry: crate::nurbs::note_refusal(
            NurbsSurface::from_checked_lanes(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    control_points
                        .chunks(v_count as usize)
                        .map(<[_]>::to_vec)
                        .collect(),
                    weights
                        .map(|values| values.chunks(v_count as usize).map(<[_]>::to_vec).collect()),
                ),
                false,
            ),
            refusal,
            format_args!("a5 NURBS surface record at byte {pos}"),
        )?,
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
    {
        return None;
    }
    let u_knots = A8KnotLane::try_new(u_distinct, u_mults)?;
    let v_knots = A8KnotLane::try_new(v_distinct, v_mults)?;
    let u_count = u_knots.pole_count(u_degree)?;
    let v_count = v_knots.pole_count(v_degree)?;
    if u_count == 0 || v_count == 0 {
        return None;
    }
    let tail_end = at.checked_add(141)?;
    let elided = tail_end <= end
        && closed_a8_child_run(data, tail_end, end)
        && parse_a8_elided_surface_tail(data, at, v_knots.distinct()).is_some();
    Some(ParsedA8SurfaceHeader {
        header: A8SurfaceHeader {
            pos,
            object_id,
            u_degree,
            v_degree,
            u_knots,
            v_knots,
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

fn a8_surface_from_parsed(
    data: &[u8],
    parsed: ParsedA8SurfaceHeader,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<FreeformSurface> {
    let ParsedA8SurfaceHeader {
        header,
        mut pole_start,
        end,
    } = parsed;
    let u_count = header.u_count()?;
    let v_count = header.v_count()?;
    let A8SurfaceHeader {
        pos,
        object_id,
        u_degree,
        v_degree,
        u_knots,
        v_knots,
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
    let weights = if rational {
        f64_values(data, &mut pole_start, poles, end)?
            .into_iter()
            .map(|weight| NonZeroReal::new(weight.get()))
            .collect::<Option<Vec<_>>>()?
    } else {
        Vec::new()
    };
    a8_surface_suffix_start(data, pole_start, end)?;
    Some(FreeformSurface {
        pos,
        identity: Some(object_id),
        geometry: crate::nurbs::note_refusal(
            NurbsSurface::from_checked_lanes(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    u_degree,
                    u_knots.expanded()?,
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    v_degree,
                    v_knots.expanded()?,
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    control_points
                        .chunks(v_count as usize)
                        .map(<[_]>::to_vec)
                        .collect(),
                    rational
                        .then_some(weights)
                        .map(|values| values.chunks(v_count as usize).map(<[_]>::to_vec).collect()),
                ),
                false,
            ),
            refusal,
            format_args!("a8 NURBS surface record #{object_id} at byte {pos}"),
        )?,
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

/// Read `count` finite little-endian `f64` values that end at or before `end`.
fn f64_values(bytes: &[u8], at: &mut usize, count: usize, end: usize) -> Option<Vec<FiniteReal>> {
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

fn a5_knots(distinct: &[f64], degree: u32) -> Option<(Vec<f64>, u32)> {
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

fn a5_weights(
    bytes: &[u8],
    at: &mut usize,
    rows: usize,
    cols: usize,
    end: usize,
) -> Option<Vec<NonZeroReal>> {
    let count = rows.checked_mul(cols)?;
    if bytes.get(*at) == Some(&0x00) {
        *at += 1;
        return f64_values(bytes, at, count, end)?
            .into_iter()
            .map(|weight| NonZeroReal::new(weight.get()))
            .collect();
    }
    if bytes.get(*at) != Some(&0x01) {
        return None;
    }

    let seed_count = cols.div_ceil(2);
    let mut weights = Vec::with_capacity(count);
    let mut previous = None::<Vec<NonZeroReal>>;
    for _ in 0..rows {
        let row = if bytes.get(*at) == Some(&0x02) {
            *at += 1;
            previous.clone()?
        } else {
            if !matches!(bytes.get(*at..*at + 3), Some([0x01, 0x03 | 0x07, 0x00])) {
                return None;
            }
            *at += 3;
            let seed = f64_values(bytes, at, seed_count, end)?
                .into_iter()
                .map(|weight| NonZeroReal::new(weight.get()))
                .collect::<Option<Vec<_>>>()?;
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
    Some(weights)
}

#[cfg(test)]
mod tests;
