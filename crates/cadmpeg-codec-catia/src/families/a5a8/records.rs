//! A-family consolidated curve and surface record vocabulary.
//!
//! Decodes `a5`/`a8` NURBS surface carriers, common-form and consolidated
//! rolling-ball jets, guide-curve jets, and object-stream UV pcurves.

use super::knot_lane::A8KnotLane;
use crate::math::distance;
use crate::nurbs::pole_count;
use crate::wire::bytes::{
    compact_int, f64_le, f64_point, read_f64_array, u32_le_24,
};
#[cfg(test)]
use crate::wire::records::ConsolidatedPcurve;
use crate::wire::records::{
    consolidated_records, ConsolidatedFamily, ConsolidatedFrame, ConsolidatedRecord,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
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

fn a8_frames(data: &[u8], class: u8) -> impl Iterator<Item = A8Frame> + '_ {
    let mut pos = 0usize;
    std::iter::from_fn(move || loop {
        if pos.checked_add(11).is_none_or(|end| end > data.len()) {
            return None;
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
        let frame = A8Frame {
            pos,
            payload: pos + 11,
            end,
            object_id,
        };
        pos = end;
        if data[frame.pos + 2] == class {
            return Some(frame);
        }
    })
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
    let layout = scan_a8_surface_layout(
        data,
        A8Frame {
            pos: frame_start,
            payload: payload_start,
            end: frame_end,
            object_id: frame.object_id,
        },
    )?;
    let suffix_start = if layout.pole_storage == PoleStorage::Elided {
        layout.pole_start.checked_add(141)?
    } else {
        let poles = crate::nurbs_surface_control_count(
            usize::try_from(layout.u.poles).ok()?,
            usize::try_from(layout.v.poles).ok()?,
        )?;
        let pole_bytes = poles.checked_mul(24)?;
        let weight_bytes = if layout.rational {
            poles.checked_mul(8)?
        } else {
            0
        };
        layout
            .pole_start
            .checked_add(pole_bytes)?
            .checked_add(weight_bytes)?
    };
    let child_start = a8_surface_suffix_start(data, suffix_start, frame_end)?;
    (child_start < frame_end).then_some(child_start)
}

fn parse_a8_elided_surface_tail(data: &[u8], at: usize, expected_v_span: f64) -> Option<usize> {
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
    let continuation = tail.get(continuation_start..continuation_end)?;
    for index in 0..continuation_bytes / 8 {
        let value = f64_le(continuation, index * 8)?;
        if tail_len == 133 && value.get() != 0.0 {
            return None;
        }
    }
    let suffix = &tail[continuation_end..];
    let valid_suffix = match (tail_len, &tail[68..71]) {
        (133, [0x01, 0x01, 0x01]) => suffix == [0x01, 0x00, 0x01, 0x00, 0x07, 0x07],
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

fn object_stream_frames(data: &[u8]) -> impl Iterator<Item = ObjectStreamFrame> + '_ {
    let mut pos = 0usize;
    let mut child_end = None::<usize>;
    let mut resume = 0usize;
    std::iter::from_fn(move || loop {
        let limit = child_end.unwrap_or(data.len());
        if pos.checked_add(8).is_none_or(|end| end > limit) {
            if child_end.take().is_some() {
                pos = resume;
                continue;
            }
            return None;
        }
        let Some(frame) = object_stream_frame(data, pos).filter(|frame| frame.end <= limit) else {
            pos += 1;
            continue;
        };
        match frame.family {
            0xa8 if child_end.is_none() => {
                child_end = Some(frame.end);
                resume = frame.end;
                pos = frame.payload;
                return Some(frame);
            }
            0xb5 => {
                pos = frame.end;
                return Some(frame);
            }
            _ => pos += 1,
        }
    })
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

    pub(in crate::families) fn knots(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<FiniteReal>, cadmpeg_core::CodecError> {
        let mut knots = Vec::new();
        crate::resource::reserve_vec(ctx, &mut knots, self.sites.len(), "catia A8 pcurve distinct knots")?;
        knots.extend(self.sites.iter().map(|site| site.knot));
        Ok(knots)
    }

    #[cfg(test)]
    fn points(&self) -> Vec<[f64; 2]> {
        self.sites.iter().map(|site| site.point.get()).collect()
    }

    pub(in crate::families) fn bspline(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<(Vec<f64>, Vec<FiniteVector<2>>)>, cadmpeg_core::CodecError> {
        let mut knots = Vec::new();
        let mut points = Vec::new();
        let mut first = Vec::new();
        let mut second = Vec::new();
        crate::resource::reserve_vec(ctx, &mut knots, self.sites.len(), "catia A8 pcurve jet knots")?;
        crate::resource::reserve_vec(ctx, &mut points, self.sites.len(), "catia A8 pcurve jet points")?;
        crate::resource::reserve_vec(ctx, &mut first, self.sites.len(), "catia A8 pcurve first jets")?;
        crate::resource::reserve_vec(ctx, &mut second, self.sites.len(), "catia A8 pcurve second jets")?;
        for site in &self.sites {
            knots.push(site.knot.get());
            points.push(site.point.get());
            first.push(site.first_derivative.get());
            second.push(site.second_derivative.get());
        }
        crate::nurbs::quintic_jet_bspline(
            ctx,
            Self::DEGREE,
            &knots,
            &points,
            &first,
            &second,
        )
    }
}

/// Decode framed `a5 03 20` consolidated UV jets.
#[must_use]
#[cfg(test)]
fn a5_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        crate::wire::records::family_pcurves_from_records(ctx, data, &records, ConsolidatedFamily::A)
            .expect("service decode")
    })
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

    fn knots(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
        let mut knots = Vec::new();
        crate::resource::reserve_vec(ctx, &mut knots, self.sites.len(), "catia A5 rolling ball knots")?;
        knots.extend(self.sites.iter().map(|site| site.knot.get()));
        Ok(knots)
    }
}

/// Lower either limiting locus of a complete rolling-ball jet to its exact
/// degree-5 NURBS representation.
pub(in crate::families) fn rolling_ball_limit_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    jet: &A5FreeformCurve,
    second_limit: bool,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<NurbsCurve>, cadmpeg_core::CodecError> {
    let offset = usize::from(second_limit) * 3;
    let mut positions = Vec::new();
    let mut first = Vec::new();
    let mut second = Vec::new();
    crate::resource::reserve_vec(ctx, &mut positions, jet.sites.len(), "catia A5 rolling ball positions")?;
    crate::resource::reserve_vec(ctx, &mut first, jet.sites.len(), "catia A5 rolling ball first jets")?;
    crate::resource::reserve_vec(ctx, &mut second, jet.sites.len(), "catia A5 rolling ball second jets")?;
    for sample in &jet.sites {
            let limit = if second_limit {
                sample.site.limit2
            } else {
                sample.site.limit1
            };
            positions.push(limit.get().into());
            let values = FiniteReal::raw_array(sample.first_derivatives);
            first.push([values[offset], values[offset + 1], values[offset + 2]]);
            let values = FiniteReal::raw_array(sample.second_derivatives);
            second.push([values[offset], values[offset + 1], values[offset + 2]]);
    }
    let knots = jet.knots(ctx)?;
    let Some((knots, control_points)) = crate::nurbs::quintic_jet_bspline(
        ctx,
        A5FreeformCurve::DEGREE,
        &knots,
        &positions,
        &first,
        &second,
    )? else {
        refusal.push_solver(
            format_args!(
                "consolidated_a5_03_32 rolling-ball limit curve at byte {}",
                jet.pos
            ),
            "states knot-aligned jet samples the degree-5 B-spline lowering does not close",
        );
        return Ok(None);
    };
    let mut poles = Vec::new();
    crate::resource::reserve_vec(ctx, &mut poles, control_points.len(), "catia A5 rolling ball poles")?;
    poles.extend(control_points.into_iter().map(|point| Point3::new(point[0], point[1], point[2])));
    Ok(crate::nurbs::note_refusal(
        NurbsCurve::from_lanes(
            A5FreeformCurve::DEGREE,
            knots,
            poles,
            None,
            false,
        ),
        refusal,
        format_args!(
            "consolidated_a5_03_32 rolling-ball limit curve at byte {}",
            jet.pos
        ),
    ))
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
    pub(in crate::families) fn knots(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
        let mut knots = Vec::new();
        crate::resource::reserve_vec(ctx, &mut knots, self.sites.len(), "catia A5 guide knots")?;
        knots.extend(self.sites.iter().map(|site| site.knot.get()));
        Ok(knots)
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
fn a5_nurbs_curves(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<Vec<A5NurbsCurve>, CodecError> {
    let records = consolidated_records(data);
    a5_nurbs_curves_from_records(ctx, data, &records, &mut crate::nurbs::LaneRefusals::new())
}

pub(in crate::families) fn a5_nurbs_curves_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<A5NurbsCurve>, CodecError> {
    let mut curves = Vec::new();
    for record in records.iter().filter(|record| {
        record.family == ConsolidatedFamily::A && record.class == 0x16
    }) {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else { continue };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(), payload: payload.start, end: end.end,
            header_token: record.header_token,
        };
        if let Some(curve) = parse_a5_nurbs_curve(ctx, data, frame, refusal)? {
            crate::resource::push(ctx, &mut curves, curve, "catia_a5_nurbs_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a5_nurbs_curve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: ConsolidatedFrame,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<A5NurbsCurve>, CodecError> {
    let Some((degree, knot_count, control_count, knot_start, control_start)) = (|| {
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
    let knot_start = at;
    let mut previous_knot = None;
    for _ in 0..knot_count {
        let knot = f64_le(data, at)?.get();
        if previous_knot.is_some_and(|previous| knot <= previous) {
            return None;
        }
        previous_knot = Some(knot);
        at += 8;
    }
    if data.get(at) != Some(&0x01) {
        return None;
    }
    at += 1;
    let control_start = at;
    for _ in 0..control_count {
        f64_point(data, at)?;
        at += 24;
    }
    if compact_int(data, &mut at)? != 1 || compact_int(data, &mut at)? != 2 {
        return None;
    }
    let range_origin = f64_le(data, at)?.get();
    let repeated_end = f64_le(data, at + 8)?.get();
    let scale = f64_le(data, at + 16)?.get();
    let offset = f64_le(data, at + 24)?.get();
    at += 32;
    if range_origin.to_bits() != 0.0f64.to_bits()
        || repeated_end.to_bits() != previous_knot?.to_bits()
        || scale.to_bits() != 1.0f64.to_bits()
        || offset.to_bits() != 0.0f64.to_bits()
        || data.get(at..frame.end) != Some(&[0x00, 0x07])
    {
        return None;
    }
    Some((degree, knot_count, control_count, knot_start, control_start))
    })() else { return Ok(None) };
    let mut distinct_knots = Vec::new();
    crate::resource::reserve_vec(ctx, &mut distinct_knots, knot_count, "catia_a5_nurbs_distinct_knots")?;
    for index in 0..knot_count {
        let Some(at) = knot_start.checked_add(index * 8) else { return Ok(None) };
        let Some(knot) = f64_le(data, at) else { return Ok(None) };
        distinct_knots.push(knot.get());
    }
    let mut control_points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut control_points, control_count, "catia_a5_nurbs_control_points")?;
    for index in 0..control_count {
        let Some(at) = control_start.checked_add(index * 24) else { return Ok(None) };
        let Some(point) = f64_point(data, at) else { return Ok(None) };
        control_points.push(point);
    }
    let Some(expanded_count) = usize::try_from(degree).ok()
        .and_then(|degree| control_count.checked_add(degree))
        .and_then(|count| count.checked_add(1)) else { return Ok(None) };
    let mut knots = Vec::new();
    crate::resource::reserve_vec(ctx, &mut knots, expanded_count, "catia_a5_nurbs_expanded_knots")?;
    for (index, knot) in distinct_knots.into_iter().enumerate() {
        let multiplicity = if index == 0 || index + 1 == knot_count {
            6
        } else {
            3
        };
        knots.extend(std::iter::repeat_n(knot, multiplicity));
    }
    Ok(crate::nurbs::note_refusal(
        NurbsCurve::from_lanes(degree, knots, control_points, None, false),
        refusal,
        format_args!("a5 NURBS curve record at byte {}", frame.pos),
    ).map(|geometry| A5NurbsCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        geometry,
    }))
}

/// Decode `a5/a6/a7 03 39` guide-curve and unit-direction jets.
#[must_use]
#[cfg(test)]
fn a5_guide_curves(
    ctx: &DecodeContext<'_>, data: &[u8],
) -> Result<Vec<A5GuideCurve>, CodecError> {
    let records = consolidated_records(data);
    a5_guide_curves_from_records(ctx, data, &records)
}

pub(in crate::families) fn a5_guide_curves_from_records(
    ctx: &DecodeContext<'_>, data: &[u8], records: &[ConsolidatedRecord],
) -> Result<Vec<A5GuideCurve>, CodecError> {
    let mut curves = Vec::new();
    for record in records.iter().filter(|record| {
        record.family == ConsolidatedFamily::A && record.class == 0x39
    }) {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else { continue };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(), payload: payload.start, end: end.end,
            header_token: record.header_token,
        };
        if let Some(curve) = parse_a5_guide_curve(ctx, data, frame)? {
            crate::resource::push(ctx, &mut curves, curve, "catia_a5_guide_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a5_guide_curve(
    ctx: &DecodeContext<'_>, data: &[u8], frame: ConsolidatedFrame,
) -> Result<Option<A5GuideCurve>, CodecError> {
    let mut at = frame.payload;
    let Some(count) = compact_int(data, &mut at).and_then(|value| usize::try_from(value).ok())
        else { return Ok(None) };
    let Some(degree) = compact_int(data, &mut at) else { return Ok(None) };
    if compact_int(data, &mut at).and_then(|value| usize::try_from(value).ok()) != Some(count)
        || count < 2 || !(1..=9).contains(&degree)
    {
        return Ok(None);
    }
    let Some(next) = consume_array_marker(data, at) else { return Ok(None) };
    at = next;
    let Some(block_bytes) = count.checked_mul(48) else { return Ok(None) };
    let Some(known_bytes) = count.checked_mul(8)
        .and_then(|bytes| block_bytes.checked_mul(3).and_then(|blocks| bytes.checked_add(blocks)))
        .and_then(|bytes| bytes.checked_add(48)) else { return Ok(None) };
    if at.checked_add(known_bytes).is_none_or(|last| last > frame.end) {
        return Ok(None);
    }
    let knot_start = at;
    let Some(first_block) = at.checked_add(count * 8) else { return Ok(None) };
    let Some(second_block) = first_block.checked_add(block_bytes) else { return Ok(None) };
    let Some(third_block) = second_block.checked_add(block_bytes) else { return Ok(None) };
    if third_block.checked_add(block_bytes).and_then(|last| last.checked_add(48)) != Some(frame.end) {
        return Ok(None);
    }
    let mut previous = None;
    for index in 0..count {
        let Some(knot) = f64_le(data, knot_start + index * 8) else { return Ok(None) };
        if previous.is_some_and(|value| value >= knot.get()) { return Ok(None) }
        previous = Some(knot.get());
    }
    let mut sites = Vec::new();
    crate::resource::reserve_vec(ctx, &mut sites, count, "catia_a5_guide_sites")?;
    for index in 0..count {
        let offset = index * 48;
        let (Some(value), Some(first_derivative), Some(second_derivative), Some(knot)) = (
            read_f64_array::<6>(data, first_block + offset),
            read_f64_array::<6>(data, second_block + offset),
            read_f64_array::<6>(data, third_block + offset),
            f64_le(data, knot_start + index * 8),
        ) else { return Ok(None) };
        let direction = [
            value[3].get() - value[0].get(),
            value[4].get() - value[1].get(),
            value[5].get() - value[2].get(),
        ];
        let length = (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
        if !((length - 1.0).abs() < EPS_GUIDE_DIRECTION_UNIT) { return Ok(None) }
        sites.push(GuideCurveSite {
            knot,
            first_derivative: first_derivative.into(),
            second_derivative: second_derivative.into(),
            point: [value[0], value[1], value[2]].into(),
        });
    }
    Ok(Some(A5GuideCurve {
        pos: frame.pos, header_token: frame.header_token, degree, sites,
    }))
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

    pub(in crate::families) fn multiplicities(
        &self, ctx: &DecodeContext<'_>,
    ) -> Result<Vec<u32>, CodecError> {
        let mut multiplicities = Vec::new();
        crate::resource::reserve_vec(ctx, &mut multiplicities, self.sites.len(), "catia_a8_jet_multiplicities")?;
        multiplicities.extend(self.sites.iter().map(|site| site.multiplicity));
        Ok(multiplicities)
    }
}

/// Convert a complete common-form rolling-ball jet to its exact neutral
/// procedural carrier.
pub(in crate::families) fn rolling_ball_jet_definition(
    ctx: &DecodeContext<'_>,
    jet: &A8FreeformCurve,
) -> Result<Option<ProceduralSurfaceDefinition>, CodecError> {
    if jet.sites.is_empty() {
        return Ok(None);
    }
    let mut stations = Vec::new();
    crate::resource::reserve_vec(ctx, &mut stations, jet.sites.len(), "catia_a8_jet_stations")?;
    stations.extend(jet.sites.iter().map(|sample| cadmpeg_ir::geometry::RollingBallJetStation {
            knot: sample.knot,
            multiplicity: sample.multiplicity,
            site: rolling_ball_jet_site(
                &sample.site,
                sample.first_derivatives,
                sample.second_derivatives,
            ),
        }));
    Ok(cadmpeg_ir::geometry::RollingBallJetStations::from_admitted(
            A8FreeformCurve::DEGREE,
            stations,
        )
        .ok()
        .map(ProceduralSurfaceDefinition::RollingBallJet))
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
pub(in crate::families) fn a8_freeform_curves(
    ctx: &DecodeContext<'_>, data: &[u8],
) -> Result<Vec<A8FreeformCurve>, CodecError> {
    let mut curves = Vec::new();
    for frame in a8_frames(data, 0x32) {
        if let Some(curve) = parse_a8_curve(ctx, data, frame)? {
            crate::resource::push(ctx, &mut curves, curve, "catia_a8_freeform_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a8_curve(
    ctx: &DecodeContext<'_>, data: &[u8], frame: A8Frame,
) -> Result<Option<A8FreeformCurve>, CodecError> {
    let A8Frame {
        pos,
        payload,
        end,
        object_id,
    } = frame;
    let Some((count, knot_start, multiplicity_start, block_start, block_bytes)) = (|| {
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
    let knot_start = at;
    let mut previous_knot = None;
    for _ in 0..count {
        let knot = f64_le(data, at)?.get();
        if previous_knot.is_some_and(|previous| knot <= previous) { return None }
        previous_knot = Some(knot);
        at += 8;
    }
    let multiplicity_start = at;
    for index in 0..count {
        let multiplicity = compact_int(data, &mut at)?;
        if (index == 0 || index + 1 == count) && multiplicity != 6 { return None }
        if index != 0 && index + 1 != count && !matches!(multiplicity, 1 | 3) { return None }
    }
    let block_start = at;
    let blocks_end = at.checked_add(block_bytes.checked_mul(3)?)?;
    if blocks_end > end || end - blocks_end != 59 {
        return None;
    }
    Some((count, knot_start, multiplicity_start, block_start, block_bytes))
    })() else { return Ok(None) };
    let mut sites = Vec::new();
    crate::resource::reserve_vec(ctx, &mut sites, count, "catia_a8_freeform_sites")?;
    let mut multiplicity_at = multiplicity_start;
    for index in 0..count {
        let offset = index * 80;
        let (Some(knot), Some(multiplicity), Some(positions), Some(first_derivatives), Some(second_derivatives)) = (
            f64_le(data, knot_start + index * 8),
            compact_int(data, &mut multiplicity_at),
            read_f64_array::<10>(data, block_start + offset),
            read_f64_array::<10>(data, block_start + block_bytes + offset),
            read_f64_array::<10>(data, block_start + 2 * block_bytes + offset),
        ) else { return Ok(None) };
        let Some(site) = rolling_ball_site(positions) else { return Ok(None) };
        sites.push(A8FreeformJet { knot, multiplicity, site, first_derivatives, second_derivatives });
    }
    Ok(Some(A8FreeformCurve { pos, object_id, sites }))
}

/// Decode framed `a5 03 32` rolling-ball jet records.
#[must_use]
#[cfg(test)]
fn a5_freeform_curves(
    ctx: &DecodeContext<'_>, data: &[u8],
) -> Result<Vec<A5FreeformCurve>, CodecError> {
    let records = consolidated_records(data);
    a5_freeform_curves_from_records(ctx, data, &records)
}

pub(in crate::families) fn a5_freeform_curves_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<A5FreeformCurve>, CodecError> {
    let mut curves = Vec::new();
    for record in records.iter().filter(|record| {
        record.family == ConsolidatedFamily::A && record.class == 0x32
    }) {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else { continue };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(), payload: payload.start, end: end.end,
            header_token: record.header_token,
        };
        if let Some(curve) = parse_a5_curve(ctx, data, frame)? {
            crate::resource::push(ctx, &mut curves, curve, "catia_a5_freeform_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a5_curve(
    ctx: &DecodeContext<'_>, data: &[u8], frame: ConsolidatedFrame,
) -> Result<Option<A5FreeformCurve>, CodecError> {
    let ConsolidatedFrame {
        pos,
        payload,
        end,
        header_token,
    } = frame;
    let Some((count, knot_start, block_start, block_bytes)) = (|| {
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
    let knot_start = at;
    let mut previous_knot = None;
    for _ in 0..count {
        let knot = f64_le(data, at)?.get();
        if previous_knot.is_some_and(|previous| knot <= previous) { return None }
        previous_knot = Some(knot);
        at += 8;
    }
    Some((count, knot_start, at, block_bytes))
    })() else { return Ok(None) };
    let mut sites = Vec::new();
    crate::resource::reserve_vec(ctx, &mut sites, count, "catia_a5_freeform_sites")?;
    for index in 0..count {
        let offset = index * 80;
        let (Some(knot), Some(positions), Some(first_derivatives), Some(second_derivatives)) = (
            f64_le(data, knot_start + index * 8),
            read_f64_array::<10>(data, block_start + offset),
            read_f64_array::<10>(data, block_start + block_bytes + offset),
            read_f64_array::<10>(data, block_start + 2 * block_bytes + offset),
        ) else { return Ok(None) };
        let Some(site) = rolling_ball_site(positions) else { return Ok(None) };
        sites.push(A5FreeformJet { knot, site, first_derivatives, second_derivatives });
    }
    Ok(Some(A5FreeformCurve { pos, header_token, sites }))
}

fn rolling_ball_site(values: [FiniteReal; 10]) -> Option<RollingBallSite> {
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
        Some(RollingBallSite {
            limit1,
            limit2,
            center,
            theta,
        })
}

/// Decode framed `a8 <flag> 20` UV jet records.
#[must_use]
#[cfg(test)]
fn a8_pcurves(
    ctx: &DecodeContext<'_>, data: &[u8],
) -> Result<Vec<A8Pcurve>, CodecError> {
    let mut pcurves = Vec::new();
    for frame in object_stream_frames(data)
        .filter(|frame| frame.class == 0x20 && data.get(frame.pos) == Some(&0xa8))
    {
        if let Some(pcurve) = parse_object_stream_pcurve(ctx, data, frame.payload, frame.end, frame.object_id)? {
            crate::resource::push(ctx, &mut pcurves, pcurve, "catia_a8_pcurves")?;
        }
    }
    Ok(pcurves)
}

/// Decode framed `a8 <flag> 20` and `b5 <flag> 20` object-stream UV jet records.
#[must_use]
pub(in crate::families) fn object_stream_pcurves(
    ctx: &DecodeContext<'_>, data: &[u8],
) -> Result<Vec<A8Pcurve>, CodecError> {
    let mut pcurves = Vec::new();
    for frame in object_stream_frames(data).filter(|frame| frame.class == 0x20) {
        if let Some(pcurve) = parse_object_stream_pcurve(ctx, data, frame.payload, frame.end, frame.object_id)? {
            crate::resource::push(ctx, &mut pcurves, pcurve, "catia_object_stream_pcurves")?;
        }
    }
    Ok(pcurves)
}

fn parse_object_stream_pcurve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    payload: usize,
    end: usize,
    object_id: u32,
) -> Result<Option<A8Pcurve>, CodecError> {
    let Some((support_id, mode, count, knot_start, array_starts, range)) = (|| {
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
    let scan_finite = |at: &mut usize| -> Option<usize> {
        let start = *at;
        for _ in 0..count {
            f64_le(data, *at)?;
            *at += 8;
        }
        Some(start)
    };
    let knot_start = at;
    let mut previous_knot = None;
    for _ in 0..count {
        let knot = f64_le(data, at)?;
        if previous_knot.is_some_and(|previous| knot <= previous) { return None }
        previous_knot = Some(knot);
        at += 8;
    }
    for index in 0..count {
        let multiplicity = compact_int(data, &mut at)?;
        if (index == 0 || index + 1 == count) && multiplicity != 6 { return None }
        if index != 0 && index + 1 != count && multiplicity != 3 { return None }
    }
    if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
        return None;
    }
    let mode = *data.get(at)?;
    at += 1;
    if at.checked_add(array_bytes.checked_add(18)?)? > end {
        return None;
    }
    let u = scan_finite(&mut at)?;
    let v = scan_finite(&mut at)?;
    let du = scan_finite(&mut at)?;
    let dv = scan_finite(&mut at)?;
    if data.get(at) != Some(&0x05) {
        return None;
    }
    at += 1;
    let ddu = scan_finite(&mut at)?;
    let ddv = scan_finite(&mut at)?;
    let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
    at += 16;
    if data.get(at) != Some(&0x07)
        || mode % 4 != 1
        || range[0] >= range[1]
        || end != at + 1
    {
        return None;
    }
    Some((support_id, mode, count, knot_start, [u, v, du, dv, ddu, ddv], range))
    })() else { return Ok(None) };
    let mut sites = Vec::new();
    crate::resource::reserve_vec(ctx, &mut sites, count, "catia_object_stream_pcurve_sites")?;
    for index in 0..count {
        let offset = index * 8;
        let Some(knot) = f64_le(data, knot_start + offset) else { return Ok(None) };
        let mut values = [knot; 6];
        for (slot, start) in values.iter_mut().zip(array_starts) {
            let Some(value) = f64_le(data, start + offset) else { return Ok(None) };
            *slot = value;
        }
        sites.push(A8PcurveSite {
            knot,
            point: [values[0], values[1]].into(),
            first_derivative: [values[2], values[3]].into(),
            second_derivative: [values[4], values[5]].into(),
        });
    }
    Ok(Some(A8Pcurve {
        object_id,
        support_id,
        #[cfg(test)]
        mode,
        sites,
        range,
    }))
}

/// Decode common-form object-stream NURBS surfaces.  Every variable-length
/// field is bounded by the record's `payload_len`, so signature collisions do
/// not become carriers.
pub(crate) fn a8_surfaces(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let mut surfaces = Vec::new();
    for frame in a8_frames(data, 0x34) {
        let Some(parsed) = parse_a8_surface_header(ctx, data, frame)? else { continue };
        if let Some(surface) = a8_surface_from_parsed(ctx, data, parsed, refusal)? {
            crate::resource::push(ctx, &mut surfaces, surface, "catia_a8_inline_surfaces")?;
        }
    }
    Ok(surfaces)
}

/// Decode every complete common-form object-stream NURBS surface, including
/// parameter records whose pole grids occupy a uniquely bounded external
/// allocation.
#[must_use]
pub(in crate::families) fn resolved_a8_surfaces(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let mut surfaces = Vec::new();
    for frame in a8_frames(data, 0x34) {
        if let Some(surface) = resolved_a8_surface_from_object_frame(
                ctx,
                data,
                frame.pos,
                frame.end,
                frame.object_id,
                refusal,
            )? {
            crate::resource::push(ctx, &mut surfaces, surface, "catia_a8_resolved_surfaces")?;
        }
    }
    Ok(surfaces)
}

/// Decode every structurally complete `a8 <flag> 34` parameter lattice, including
/// records whose pole representation is not inline.
#[cfg(test)]
fn a8_surface_headers<'a>(
    ctx: &'a DecodeContext<'_>, data: &'a [u8],
) -> impl Iterator<Item = Result<A8SurfaceHeader, CodecError>> + 'a {
    a8_frames(data, 0x34)
        .filter_map(move |frame| {
            match a8_surface_header_from_object_frame(ctx, data, frame.pos, frame.end, frame.object_id) {
                Ok(Some(header)) => Some(Ok(header)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })
}

/// Decode one selected `a8 <flag> 34` frame's parameter lattice.
pub(in crate::families) fn a8_surface_header_from_object_frame(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Result<Option<A8SurfaceHeader>, CodecError> {
    Ok(parse_selected_a8_surface_header(ctx, data, start, end, object_id)?.map(|parsed| parsed.header))
}

/// Decode one selected `a8 <flag> 34` frame and its complete pole grid.
pub(in crate::families) fn resolved_a8_surface_from_object_frame(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FreeformSurface>, CodecError> {
    let Some(parsed) = parse_selected_a8_surface_header(ctx, data, start, end, object_id)? else {
        return Ok(None);
    };
    if parsed.header.pole_storage == PoleStorage::Elided {
        a8_surface_from_external_grid(ctx, data, &parsed.header, refusal)
    } else {
        a8_surface_from_parsed(ctx, data, parsed, refusal)
    }
}

/// Resolve an elided-pole `a8 <flag> 34` carrier from its support-referenced
/// external grid allocation. The allocation occupies the complete unframed gap
/// between a length-closed `b5 <flag> 21` pcurve and the following A/B-family
/// frame; its pcurve support reference must equal the surface object id.
#[must_use]
fn a8_surface_from_external_grid(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    header: &A8SurfaceHeader,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FreeformSurface>, CodecError> {
    let Some(need) = ExternalGridNeed::from_header(header) else { return Ok(None) };
    let mut ranges = a8_external_grid_candidate_ranges(data, need);
    let Some(range) = ranges.next() else { return Ok(None) };
    if ranges.next().is_some() { return Ok(None) }
    let Some(ExternalGridCandidate {
        control_points,
        weights,
        ..
    }) = parse_external_grid_candidate(ctx, data, header, range)? else { return Ok(None) };
    let Some(row_len) = header.v_count().and_then(|count| usize::try_from(count).ok()) else { return Ok(None) };
    let Some(u_knots) = header.u_knots.expanded(ctx)? else { return Ok(None) };
    let Some(v_knots) = header.v_knots.expanded(ctx)? else { return Ok(None) };
    let control_points = grid_rows(ctx, control_points, row_len, "catia_a8_external_pole_rows")?;
    let weights = weights.map(|values| grid_rows(ctx, values, row_len,
        "catia_a8_external_weight_rows")).transpose()?;
    Ok(crate::nurbs::note_refusal(
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::from_checked_lanes(control_points, weights)
            .and_then(|poles| {
                NurbsSurface::new(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(header.u_degree, u_knots, false),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(header.v_degree, v_knots, false),
                    poles,
                    false,
                )
            }),
        refusal,
        format_args!("a8 NURBS surface record #{} at byte {}", header.object_id, header.pos),
    ).map(|geometry| FreeformSurface {
        pos: header.pos,
        identity: Some(header.object_id),
        geometry,
    }))
}

/// Return every complete support-bound external A8 pole allocation.
pub(in crate::families) fn a8_external_grid_ranges(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<Range<usize>>, CodecError> {
    let mut ranges = Vec::new();
    for frame in a8_frames(data, 0x34) {
        let Some(layout) = scan_a8_surface_layout(data, frame) else { continue };
        for range in a8_external_grid_candidate_ranges(data, ExternalGridNeed::from_layout(frame.object_id, &layout)) {
            crate::resource::push(ctx, &mut ranges, range,
                "catia_a8_external_grid_ranges")?;
        }
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    ranges.dedup();
    Ok(ranges)
}

struct ExternalGridCandidate {
    control_points: Vec<FinitePoint3>,
    weights: Option<Vec<NonZeroReal>>,
}

#[derive(Clone, Copy)]
struct ExternalGridNeed {
    object_id: u32,
    u_count: u32,
    v_count: u32,
    rational: bool,
    pole_storage: PoleStorage,
}

impl ExternalGridNeed {
    fn from_header(header: &A8SurfaceHeader) -> Option<Self> {
        Some(Self {
            object_id: header.object_id,
            u_count: header.u_count()?,
            v_count: header.v_count()?,
            rational: header.rational,
            pole_storage: header.pole_storage,
        })
    }

    fn from_layout(object_id: u32, layout: &A8SurfaceLayout) -> Self {
        Self {
            object_id,
            u_count: layout.u.poles,
            v_count: layout.v.poles,
            rational: layout.rational,
            pole_storage: layout.pole_storage,
        }
    }
}

fn a8_external_grid_candidate_ranges<'a>(
    data: &'a [u8],
    need: ExternalGridNeed,
) -> impl Iterator<Item = Range<usize>> + 'a {
    let layout = (|| {
    if need.pole_storage != PoleStorage::Elided { return None }
    let (Some(u_count), Some(v_count)) = (
        usize::try_from(need.u_count).ok(),
        usize::try_from(need.v_count).ok(),
    ) else {
        return None;
    };
    let poles = crate::nurbs_surface_control_count(u_count, v_count)?;
    let weight_bytes = if need.rational {
        poles.checked_mul(8)?
    } else {
        0
    };
    let grid_bytes = poles
        .checked_mul(24)
        .and_then(|bytes| bytes.checked_add(weight_bytes))?;
    Some((poles, grid_bytes))
    })();
    object_stream_frames(data)
        .filter(|frame| frame.family == 0xb5 && frame.class == 0x21)
        .filter(move |frame| {
            let Some(mut at) = frame.payload.checked_add(1) else {
                return false;
            };
            object_stream_reference(data, &mut at) == Some(need.object_id)
        })
    .filter_map(move |frame| {
        let (poles, grid_bytes) = layout?;
        let start = frame.end;
        let end = start.checked_add(grid_bytes)?;
        if object_stream_frame(data, end).is_none() {
            return None;
        }
        let mut at = start;
        for _ in 0..poles {
            f64_point(data, at)?;
            at += 24;
        }
        if need.rational {
            for _ in 0..poles {
                NonZeroReal::new(f64_le(data, at)?.get())?;
                at += 8;
            }
        }
        (at == end).then_some(start..end)
    })
}

fn parse_external_grid_candidate(
    ctx: &DecodeContext<'_>, data: &[u8], header: &A8SurfaceHeader, range: Range<usize>,
) -> Result<Option<ExternalGridCandidate>, CodecError> {
    let Some(poles) = header.u_count().zip(header.v_count())
        .and_then(|(u, v)| crate::nurbs_surface_control_count(usize::try_from(u).ok()?, usize::try_from(v).ok()?))
    else { return Ok(None) };
    let mut control_points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut control_points, poles, "catia_a8_external_poles")?;
    let mut at = range.start;
    for _ in 0..poles {
        let Some(point) = f64_point(data, at) else { return Ok(None) };
        control_points.push(point);
        at += 24;
    }
    let weights = if header.rational {
        let mut weights = Vec::new();
        crate::resource::reserve_vec(ctx, &mut weights, poles, "catia_a8_external_weights")?;
        for _ in 0..poles {
            let Some(weight) = f64_le(data, at).and_then(|value| NonZeroReal::new(value.get())) else { return Ok(None) };
            weights.push(weight);
            at += 8;
        }
        Some(weights)
    } else { None };
    Ok((at == range.end).then_some(ExternalGridCandidate { control_points, weights }))
}

fn grid_rows<T>(
    ctx: &DecodeContext<'_>, values: Vec<T>, row_len: usize, operation: &'static str,
) -> Result<Vec<Vec<T>>, CodecError> {
    let rows = values.len() / row_len;
    let mut result = Vec::new();
    crate::resource::reserve_vec(ctx, &mut result, rows, operation)?;
    let mut values = values.into_iter();
    for _ in 0..rows {
        let mut row = Vec::new();
        crate::resource::reserve_vec(ctx, &mut row, row_len, operation)?;
        row.extend(values.by_ref().take(row_len));
        result.push(row);
    }
    Ok(result)
}

/// Decode consolidated `a5 03 34` NURBS surface carriers.  This family uses
/// implicit clamped multiplicities instead of the explicit `a8` vectors.
pub(crate) fn a5_surfaces(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let records = consolidated_records(data);
    a5_surfaces_from_records(ctx, data, &records, refusal)
}

pub(in crate::families) fn a5_surfaces_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let mut surfaces = Vec::new();
    for record in records.iter().filter(|record| {
        record.family == ConsolidatedFamily::A && record.class == 0x34
    }) {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else { continue };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(),
            payload: payload.start,
            end: end.end,
            header_token: record.header_token,
        };
        if let Some(surface) = a5_surface(ctx, data, frame, refusal)? {
            crate::resource::push(ctx, &mut surfaces, surface, "catia_a5_surfaces")?;
        }
    }
    Ok(surfaces)
}

fn a5_surface(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: ConsolidatedFrame,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FreeformSurface>, CodecError> {
    let ConsolidatedFrame { pos, payload, end, .. } = frame;
    let mut at = payload;
    let Some(u_degree) = data.get(at).and_then(|&byte| a5_int(byte)) else { return Ok(None) };
    at += 1;
    let Some(u_distinct_count) = data.get(at).and_then(|&byte| a5_int(byte))
        .and_then(|count| usize::try_from(count).ok()) else { return Ok(None) };
    let Some(next) = a5_array_marker(data, at + 1) else { return Ok(None) };
    at = next;
    let Some(u_distinct) = a5_distinct_values(ctx, data, &mut at, u_distinct_count, end)?
        else { return Ok(None) };
    let Some(v_degree) = data.get(at).and_then(|&byte| a5_int(byte)) else { return Ok(None) };
    at += 1;
    let Some(v_distinct_count) = data.get(at).and_then(|&byte| a5_int(byte))
        .and_then(|count| usize::try_from(count).ok()) else { return Ok(None) };
    let Some(next) = a5_array_marker(data, at + 1) else { return Ok(None) };
    at = next;
    let Some(v_distinct) = a5_distinct_values(ctx, data, &mut at, v_distinct_count, end)?
        else { return Ok(None) };
    let Some(&mode) = data.get(at) else { return Ok(None) };
    at += 1;
    if !knots_strictly_increasing(&u_distinct) || !knots_strictly_increasing(&v_distinct) {
        return Ok(None);
    }
    let Some((u_knots, u_count)) = a5_knots(ctx, &u_distinct, u_degree)? else { return Ok(None) };
    let Some((v_knots, v_count)) = a5_knots(ctx, &v_distinct, v_degree)? else { return Ok(None) };
    let (Some(u_count), Some(v_count)) = (
        usize::try_from(u_count).ok(), usize::try_from(v_count).ok(),
    ) else { return Ok(None) };
    let Some(poles) = crate::nurbs_surface_control_count(u_count, v_count) else { return Ok(None) };
    if poles.checked_mul(24).and_then(|bytes| at.checked_add(bytes))
        .is_none_or(|end_poles| end_poles > end) { return Ok(None) }
    let mut control_points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut control_points, poles, "catia_a5_surface_poles")?;
    for _ in 0..poles {
        let Some(point) = f64_point(data, at) else { return Ok(None) };
        control_points.push(point);
        at += 24;
    }
    let weights = match mode {
        0x01 => None,
        0x05 => {
            let Some(weights) = a5_weights(ctx, data, &mut at, u_count, v_count, end)?
                else { return Ok(None) };
            Some(weights)
        }
        _ => return Ok(None),
    };
    if !valid_a5_surface_tail(data, at, end) { return Ok(None) }
    let control_points = grid_rows(ctx, control_points, v_count, "catia_a5_surface_pole_rows")?;
    let weights = weights.map(|values| grid_rows(ctx, values, v_count,
        "catia_a5_surface_weight_rows")).transpose()?;
    Ok(crate::nurbs::note_refusal(
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::from_checked_lanes(control_points, weights)
            .and_then(|poles| {
                NurbsSurface::new(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
                    poles,
                    false,
                )
            }),
        refusal,
        format_args!("a5 NURBS surface record at byte {pos}"),
    ).map(|geometry| FreeformSurface { pos, identity: None, geometry }))
}

struct ParsedA8SurfaceHeader {
    header: A8SurfaceHeader,
    pole_start: usize,
    end: usize,
}

fn parse_selected_a8_surface_header(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Result<Option<ParsedA8SurfaceHeader>, CodecError> {
    let Some(frame) = object_stream_frame(data, start) else { return Ok(None) };
    if !(frame.family == 0xa8
        && frame.class == 0x34
        && frame.end == end
        && frame.object_id == object_id) { return Ok(None) }
    parse_a8_surface_header(
        ctx,
        data,
        A8Frame {
            pos: start,
            payload: frame.payload,
            end,
            object_id,
        },
    )
}

#[derive(Clone, Copy)]
struct A8LaneLayout {
    distinct_start: usize,
    multiplicity_start: usize,
    count: usize,
    poles: u32,
}

struct A8SurfaceLayout {
    u_degree: u32,
    v_degree: u32,
    u: A8LaneLayout,
    v: A8LaneLayout,
    pole_start: usize,
    rational: bool,
    pole_storage: PoleStorage,
}

fn scan_a8_lane(
    data: &[u8], at: &mut usize, count: usize, degree: u32, end: usize,
) -> Option<(A8LaneLayout, f64, f64)> {
    let distinct_start = *at;
    if at.checked_add(count.checked_mul(8)?)? > end {
        return None;
    }
    let mut first = None;
    let mut last = None;
    for _ in 0..count {
        let value = f64_le(data, *at)?.get();
        if last.is_some_and(|previous| previous >= value) {
            return None;
        }
        first.get_or_insert(value);
        last = Some(value);
        *at += 8;
    }
    let multiplicity_start = *at;
    let mut total = 0u32;
    for _ in 0..count {
        total = total.checked_add(compact_int(data, at)?)?;
    }
    let poles = total.checked_sub(degree.checked_add(1)?)?;
    Some((A8LaneLayout { distinct_start, multiplicity_start, count, poles }, first?, last?))
}

fn scan_a8_surface_layout(data: &[u8], frame: A8Frame) -> Option<A8SurfaceLayout> {
    let A8Frame { payload, end, .. } = frame;
    if end.checked_sub(payload)? < 20 {
        return None;
    }
    let mut at = payload.checked_add(1)?;
    let u_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    let u_count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    at = consume_array_marker(data, at)?;
    let (u, _, _) = scan_a8_lane(data, &mut at, u_count, u_degree, end)?;
    let v_degree = compact_int(data, &mut at)?;
    at = at.checked_add(2)?;
    let v_count = usize::try_from(compact_int(data, &mut at)?).ok()?;
    at = consume_array_marker(data, at)?;
    let (v, v_first, v_last) = scan_a8_lane(data, &mut at, v_count, v_degree, end)?;
    let mode = *data.get(at)?;
    at += 1;
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || u_count < 2 || v_count < 2
        || !matches!(mode, 0x01 | 0x05)
        || u.poles == 0 || v.poles == 0
    {
        return None;
    }
    let tail_end = at.checked_add(141)?;
    let elided = tail_end <= end
        && closed_a8_child_run(data, tail_end, end)
        && parse_a8_elided_surface_tail(data, at, v_last - v_first).is_some();
    Some(A8SurfaceLayout {
        u_degree, v_degree, u, v, pole_start: at, rational: mode == 0x05,
        pole_storage: if elided { PoleStorage::Elided } else { PoleStorage::Inline },
    })
}

fn materialize_a8_lane(
    ctx: &DecodeContext<'_>, data: &[u8], layout: A8LaneLayout,
) -> Result<Option<A8KnotLane>, CodecError> {
    let mut distinct = Vec::new();
    crate::resource::reserve_vec(ctx, &mut distinct, layout.count, "catia_a8_distinct_knots")?;
    let mut at = layout.distinct_start;
    for _ in 0..layout.count {
        let Some(value) = f64_le(data, at) else { return Ok(None) };
        distinct.push(value);
        at += 8;
    }
    let mut multiplicities = Vec::new();
    crate::resource::reserve_vec(ctx, &mut multiplicities, layout.count, "catia_a8_multiplicities")?;
    at = layout.multiplicity_start;
    for _ in 0..layout.count {
        let Some(value) = compact_int(data, &mut at) else { return Ok(None) };
        multiplicities.push(value);
    }
    Ok(A8KnotLane::try_new(distinct, multiplicities))
}

fn parse_a8_surface_header(
    ctx: &DecodeContext<'_>, data: &[u8], frame: A8Frame,
) -> Result<Option<ParsedA8SurfaceHeader>, CodecError> {
    let Some(layout) = scan_a8_surface_layout(data, frame) else { return Ok(None) };
    let Some(u_knots) = materialize_a8_lane(ctx, data, layout.u)? else { return Ok(None) };
    let Some(v_knots) = materialize_a8_lane(ctx, data, layout.v)? else { return Ok(None) };
    Ok(Some(ParsedA8SurfaceHeader {
        header: A8SurfaceHeader {
            pos: frame.pos,
            object_id: frame.object_id,
            u_degree: layout.u_degree,
            v_degree: layout.v_degree,
            u_knots,
            v_knots,
            rational: layout.rational,
            pole_storage: layout.pole_storage,
        },
        pole_start: layout.pole_start,
        end: frame.end,
    }))
}

fn a8_surface_from_parsed(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    parsed: ParsedA8SurfaceHeader,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FreeformSurface>, CodecError> {
    let ParsedA8SurfaceHeader {
        header,
        mut pole_start,
        end,
    } = parsed;
    let Some(u_count) = header.u_count().and_then(|count| usize::try_from(count).ok()) else { return Ok(None) };
    let Some(v_count) = header.v_count().and_then(|count| usize::try_from(count).ok()) else { return Ok(None) };
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
        return Ok(None);
    }
    let Some(poles) = crate::nurbs_surface_control_count(u_count, v_count) else { return Ok(None) };
    let Some(pole_bytes) = poles.checked_mul(24) else { return Ok(None) };
    if pole_start.checked_add(pole_bytes).is_none_or(|end_poles| end_poles > end) {
        return Ok(None);
    }
    let mut control_points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut control_points, poles, "catia_a8_inline_poles")?;
    for _ in 0..poles {
        let Some(point) = f64_point(data, pole_start) else { return Ok(None) };
        control_points.push(point);
        pole_start += 24;
    }
    let weights = if rational {
        if poles.checked_mul(8).and_then(|bytes| pole_start.checked_add(bytes))
            .is_none_or(|end_weights| end_weights > end) { return Ok(None) }
        let mut weights = Vec::new();
        crate::resource::reserve_vec(ctx, &mut weights, poles, "catia_a8_inline_weights")?;
        for _ in 0..poles {
            let Some(weight) = f64_le(data, pole_start).and_then(|value| NonZeroReal::new(value.get())) else { return Ok(None) };
            weights.push(weight);
            pole_start += 8;
        }
        weights
    } else {
        Vec::new()
    };
    if a8_surface_suffix_start(data, pole_start, end).is_none() { return Ok(None) }
    let Some(u_knots) = u_knots.expanded(ctx)? else { return Ok(None) };
    let Some(v_knots) = v_knots.expanded(ctx)? else { return Ok(None) };
    let control_points = grid_rows(ctx, control_points, v_count, "catia_a8_inline_pole_rows")?;
    let weights = rational.then(|| grid_rows(ctx, weights, v_count,
        "catia_a8_inline_weight_rows")).transpose()?;
    Ok(crate::nurbs::note_refusal(
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::from_checked_lanes(control_points, weights)
            .and_then(|poles| {
                NurbsSurface::new(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
                    poles,
                    false,
                )
            }),
        refusal,
        format_args!("a8 NURBS surface record #{object_id} at byte {pos}"),
    ).map(|geometry| FreeformSurface {
        pos,
        identity: Some(object_id),
        geometry,
    }))
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

fn a5_distinct_values(
    ctx: &DecodeContext<'_>, bytes: &[u8], at: &mut usize, count: usize, end: usize,
) -> Result<Option<Vec<f64>>, CodecError> {
    if count.checked_mul(8).and_then(|width| at.checked_add(width))
        .is_none_or(|last| last > end) { return Ok(None) }
    let mut values = Vec::new();
    crate::resource::reserve_vec(ctx, &mut values, count, "catia_a5_distinct_knots")?;
    for _ in 0..count {
        let Some(value) = f64_le(bytes, *at) else { return Ok(None) };
        values.push(value.get());
        *at += 8;
    }
    Ok(Some(values))
}

fn a5_knots(
    ctx: &DecodeContext<'_>, distinct: &[f64], degree: u32,
) -> Result<Option<(Vec<f64>, u32)>, CodecError> {
    let (endpoint, interior) = match degree {
        1 | 3 if distinct.len() >= 2 => (degree + 1, 1u32),
        5 if distinct.len() >= 2 => (6u32, 3u32),
        _ => return Ok(None),
    };
    let mut multiplicities = Vec::new();
    crate::resource::reserve_vec(ctx, &mut multiplicities, distinct.len(),
        "catia_a5_knot_multiplicities")?;
    multiplicities.push(endpoint);
    multiplicities.extend(std::iter::repeat_n(interior, distinct.len() - 2));
    multiplicities.push(endpoint);
    let Some(count) = pole_count(&multiplicities, degree) else { return Ok(None) };
    let Some(expanded_count) = multiplicities.iter().try_fold(0usize, |sum, &value| {
        sum.checked_add(usize::try_from(value).ok()?)
    }) else { return Ok(None) };
    let mut knots = Vec::new();
    crate::resource::reserve_vec(ctx, &mut knots, expanded_count, "catia_a5_expanded_knots")?;
    for (&knot, &multiplicity) in distinct.iter().zip(&multiplicities) {
        let Some(repeats) = usize::try_from(multiplicity).ok() else { return Ok(None) };
        knots.extend(std::iter::repeat_n(knot, repeats));
    }
    Ok(Some((knots, count)))
}

fn a5_weights(
    ctx: &DecodeContext<'_>, bytes: &[u8], at: &mut usize,
    rows: usize, cols: usize, end: usize,
) -> Result<Option<Vec<NonZeroReal>>, CodecError> {
    let Some(count) = rows.checked_mul(cols) else { return Ok(None) };
    if bytes.get(*at) == Some(&0x00) {
        *at += 1;
        if count.checked_mul(8).and_then(|width| at.checked_add(width))
            .is_none_or(|last| last > end) { return Ok(None) }
        let mut weights = Vec::new();
        crate::resource::reserve_vec(ctx, &mut weights, count, "catia_a5_explicit_weights")?;
        for _ in 0..count {
            let Some(weight) = f64_le(bytes, *at).and_then(|value| NonZeroReal::new(value.get()))
                else { return Ok(None) };
            weights.push(weight);
            *at += 8;
        }
        return Ok(Some(weights));
    }
    if bytes.get(*at) != Some(&0x01) { return Ok(None) }
    let seed_count = cols.div_ceil(2);
    let mut weights = Vec::new();
    crate::resource::reserve_vec(ctx, &mut weights, count, "catia_a5_mirrored_weights")?;
    for _ in 0..rows {
        if bytes.get(*at) == Some(&0x02) {
            *at += 1;
            let Some(previous_start) = weights.len().checked_sub(cols) else { return Ok(None) };
            for index in 0..cols {
                let value = weights[previous_start + index];
                weights.push(value);
            }
            continue;
        }
        if !matches!(bytes.get(*at..*at + 3), Some([0x01, 0x03 | 0x07, 0x00])) {
            return Ok(None);
        }
        *at += 3;
        if seed_count.checked_mul(8).and_then(|width| at.checked_add(width))
            .is_none_or(|last| last > end) { return Ok(None) }
        let row_start = weights.len();
        for _ in 0..seed_count {
            let Some(weight) = f64_le(bytes, *at).and_then(|value| NonZeroReal::new(value.get()))
                else { return Ok(None) };
            weights.push(weight);
            *at += 8;
        }
        for offset in (0..cols / 2).rev() {
            weights.push(weights[row_start + offset]);
        }
        if weights.len() != row_start + cols { return Ok(None) }
    }
    Ok(Some(weights))
}

#[cfg(test)]
mod tests;
