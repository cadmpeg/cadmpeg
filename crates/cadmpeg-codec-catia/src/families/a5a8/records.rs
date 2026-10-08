//! A-family consolidated curve and surface record vocabulary.
//!
//! Decodes `a5`/`a8` NURBS surface carriers, common-form and consolidated
//! rolling-ball jets, guide-curve jets, and object-stream UV pcurves.

type A8BSplineOutput = Result<Option<(Vec<f64>, Vec<FiniteVector<2>>)>, cadmpeg_core::CodecError>;
type A8PcurveKnotsAndControlsOutput =
    Result<Option<(Vec<FiniteReal>, Vec<FiniteVector<2>>)>, cadmpeg_core::CodecError>;

use super::knot_lane::A8KnotLane;
use crate::math::distance;
use crate::wire::bytes::{compact_int, f64_le, f64_point, read_f64_array, u32_le_24};
#[cfg(test)]
use crate::wire::records::{consolidated_records, ConsolidatedPcurve};
use crate::wire::records::{ConsolidatedFamily, ConsolidatedFrame, ConsolidatedRecord};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{
    nurbs::{knots_strictly_increasing, NurbsCurve, NurbsSurface},
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;
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
    payload: usize,
    end: usize,
    family: u8,
    class: u8,
    object_id: u32,
}

fn a8_frames<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    class: u8,
) -> Result<impl Iterator<Item = A8Frame> + 'a, CodecError> {
    let mut skip_until = 0usize;
    Ok(ctx
        .admit_iter(data, "catia_a8_frame_scan")?
        .enumerate()
        .filter_map(move |(pos, byte)| {
            if pos < skip_until || pos.checked_add(11).is_none_or(|end| end > data.len()) {
                return None;
            }
            if *byte != 0xa8 || !object_frame_flag(data[pos + 1]) {
                return None;
            }
            let length =
                View::u32_le_at(data, pos + 3).and_then(|value| usize::try_from(value).ok())?;
            let end = pos
                .checked_add(11)?
                .checked_add(length)
                .filter(|end| *end <= data.len())?;
            let object_id = View::u32_le_at(data, pos + 7)?;
            let frame = A8Frame {
                pos,
                payload: pos.checked_add(11)?,
                end,
                object_id,
            };
            skip_until = end;
            (data.get(pos + 2) == Some(&class)).then_some(frame)
        }))
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
        payload,
        end,
        family,
        class,
        object_id,
    })
}

fn closed_a8_child_run(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
) -> Result<bool, CodecError> {
    if start > end || end > data.len() {
        return Ok(false);
    }
    let mut at = start;
    while at < end {
        ctx.charge_work(1, "catia_a8_child_frame_scan")?;
        let Some(frame) = object_stream_frame(data, at) else {
            return Ok(false);
        };
        if frame.family != 0xb5 || frame.end > end {
            return Ok(false);
        }
        at = frame.end;
    }
    Ok(true)
}

/// Builds a decoded value under scoped storage and retains that storage only
/// when the build produces a value; a rejected candidate releases it.
fn retain_built<T>(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    build: impl FnOnce() -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    let (value, storage) = ctx.with_scoped_storage(operation, build)?;
    if value.is_some() {
        storage.commit()?;
    }
    Ok(value)
}

/// Return the start of a length-closed B5 child run owned by an A8 frame.
///
/// Common-form surface frames may place their child run after the complete
/// inline pole representation or after the fixed elided-pole tail. A marker
/// shaped byte sequence elsewhere in a surface payload is payload data and is
/// not a child run.
pub(in crate::families) fn a8_nested_b5_run_start(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame_start: usize,
    frame_end: usize,
) -> Result<Option<usize>, CodecError> {
    let Some(payload_start) = frame_start.checked_add(11) else {
        return Ok(None);
    };
    if frame_end > data.len() || payload_start > frame_end {
        return Ok(None);
    }
    if payload_start < frame_end && closed_a8_child_run(ctx, data, payload_start, frame_end)? {
        return Ok(Some(payload_start));
    }
    let Some(frame) = object_stream_frame(data, frame_start) else {
        return Ok(None);
    };
    if frame.family != 0xa8 || frame.class != 0x34 || frame.end != frame_end {
        return Ok(None);
    }
    let Some(layout) = scan_a8_surface_layout(
        ctx,
        data,
        A8Frame {
            pos: frame_start,
            payload: payload_start,
            end: frame_end,
            object_id: frame.object_id,
        },
    )?
    else {
        return Ok(None);
    };
    let Some(suffix_start) = (|| {
        if layout.pole_storage == PoleStorage::Elided {
            layout.pole_start.checked_add(141)
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
                .checked_add(weight_bytes)
        }
    })() else {
        return Ok(None);
    };
    let Some(child_start) = a8_surface_suffix_start(ctx, data, suffix_start, frame_end)? else {
        return Ok(None);
    };
    Ok((child_start < frame_end).then_some(child_start))
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
    if !matches!(tail_len, 133 | 141 | 142) {
        return None;
    }
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
    let parameters = parameters.map(FiniteReal::get);
    if parameters[0] >= parameters[1]
        || parameters[2] >= parameters[3]
        || parameters[4] == 0.0
        || parameters[6] == 0.0
    {
        return None;
    }
    // The continuation holds seven zero scalars in the short tail and eight
    // finite scalars in the long tails.
    let (continuation_valid, continuation_end) = if tail_len == 133 {
        (
            read_f64_array::<7>(tail, 71)
                .is_some_and(|values| values.iter().all(|value| value.get() == 0.0)),
            127,
        )
    } else {
        (read_f64_array::<8>(tail, 71).is_some(), 135)
    };
    if !continuation_valid {
        return None;
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

fn a8_inline_surface_tail(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    at: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    for tail_len in [133, 141, 142] {
        let Some(tail_end) = at.checked_add(tail_len).filter(|tail_end| *tail_end <= end) else {
            continue;
        };
        if parse_surface_tail(data, at, tail_end).is_some()
            && closed_a8_child_run(ctx, data, tail_end, end)?
        {
            return Ok(Some(tail_end));
        }
    }
    Ok(None)
}

fn a8_surface_suffix_start(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    at: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    if closed_a8_child_run(ctx, data, at, end)? {
        return Ok(Some(at));
    }
    a8_inline_surface_tail(ctx, data, at, end)
}

fn object_stream_frames<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
) -> Result<impl Iterator<Item = ObjectStreamFrame> + 'a, CodecError> {
    let mut child_end = None::<usize>;
    let mut resume = 0usize;
    let mut skip_until = 0usize;
    Ok(ctx
        .admit_iter(data, "catia_object_stream_frame_scan")?
        .enumerate()
        .filter_map(move |(pos, _)| {
            if pos < skip_until {
                return None;
            }
            if child_end.is_some_and(|end| pos >= end) {
                child_end = None;
                resume = 0;
            }
            let limit = child_end.unwrap_or(data.len());
            if pos.checked_add(8).is_none_or(|end| end > limit) {
                if child_end.take().is_some() {
                    skip_until = resume;
                }
                return None;
            }
            let frame = object_stream_frame(data, pos).filter(|frame| frame.end <= limit)?;
            match frame.family {
                0xa8 if child_end.is_none() => {
                    child_end = Some(frame.end);
                    resume = frame.end;
                    skip_until = frame.payload;
                    Some(frame)
                }
                0xb5 => {
                    skip_until = frame.end;
                    Some(frame)
                }
                _ => None,
            }
        }))
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
    pub(in crate::families) fn copy_charged(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            pos: self.pos,
            object_id: self.object_id,
            u_degree: self.u_degree,
            v_degree: self.v_degree,
            u_knots: self.u_knots.copy_charged(ctx)?,
            v_knots: self.v_knots.copy_charged(ctx)?,
            rational: self.rational,
            pole_storage: self.pole_storage,
        })
    }

    /// U pole count derived from degree and knot multiplicities.
    fn u_count(&self, ctx: &DecodeContext<'_>) -> Result<Option<u32>, CodecError> {
        self.u_knots.pole_count(ctx, self.u_degree)
    }

    /// V pole count derived from degree and knot multiplicities.
    fn v_count(&self, ctx: &DecodeContext<'_>) -> Result<Option<u32>, CodecError> {
        self.v_knots.pole_count(ctx, self.v_degree)
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

    #[cfg(test)]
    fn knots(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<FiniteReal>, cadmpeg_core::CodecError> {
        ctx.collect_indexed_vec(
            self.sites.len(),
            "catia_a8_pcurve_knot_projection",
            |index| Ok(self.sites[index].knot),
        )
    }

    #[cfg(test)]
    fn points(&self) -> Vec<[f64; 2]> {
        self.sites.iter().map(|site| site.point.get()).collect()
    }

    pub(in crate::families) fn bspline(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> A8BSplineOutput {
        self.with_jet_lanes(ctx, None, |ctx, knots, points, first, second| {
            crate::nurbs::quintic_jet_bspline(
                ctx,
                Self::DEGREE,
                knots,
                points,
                first,
                second,
                cadmpeg_ir::units::FiniteVector::new,
            )
        })
    }

    /// Lower the jet to controls while retaining its source knots for B5's
    /// separate multiplicity representation.
    pub(in crate::families) fn control_points_and_knots(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> A8PcurveKnotsAndControlsOutput {
        let (mut source_knots, mut source_knot_storage) =
            ctx.scoped_vector_storage(0, "catia_a8_pcurve_source_knots")?;
        ctx.reserve_scoped_vec(
            &mut source_knot_storage,
            &mut source_knots,
            self.sites.len(),
            "catia_a8_pcurve_source_knots",
        )?;
        let Some(control_points) = self.with_jet_lanes(
            ctx,
            Some(&mut source_knots),
            |ctx, knots, points, first, second| {
                crate::nurbs::quintic_jet_controls(
                    ctx,
                    Self::DEGREE,
                    knots,
                    points,
                    first,
                    second,
                    cadmpeg_ir::units::FiniteVector::new,
                )
            },
        )?
        else {
            return Ok(None);
        };
        source_knot_storage.commit()?;
        Ok(Some((source_knots, control_points)))
    }

    /// Lower the jet without retaining knots that the caller does not need.
    #[cfg(test)]
    pub(in crate::families) fn control_points(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Vec<FiniteVector<2>>>, CodecError> {
        self.with_jet_lanes(ctx, None, |ctx, knots, points, first, second| {
            crate::nurbs::quintic_jet_controls(
                ctx,
                Self::DEGREE,
                knots,
                points,
                first,
                second,
                cadmpeg_ir::units::FiniteVector::new,
            )
        })
    }

    fn with_jet_lanes<T>(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut source_knots: Option<&mut Vec<FiniteReal>>,
        project: impl FnOnce(
            &cadmpeg_core::decode::DecodeContext<'_>,
            &[f64],
            &[[f64; 2]],
            &[[f64; 2]],
            &[[f64; 2]],
        ) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        // The projected lanes are solver input and are released when it returns.
        let ((knots, points, first, second), _lanes) =
            ctx.with_scoped_storage("catia_a8_pcurve_jet_lanes", || {
                let count = self.sites.len();
                let mut knots = ctx.collection_vec(count, "catia A8 pcurve jet knots")?;
                let mut points = ctx.collection_vec(count, "catia A8 pcurve jet points")?;
                let mut first = ctx.collection_vec(count, "catia A8 pcurve first jets")?;
                let mut second = ctx.collection_vec(count, "catia A8 pcurve second jets")?;
                for site in ctx.admit_iter(&self.sites, "catia_a8_pcurve_jet_projection")? {
                    knots.push(site.knot.get());
                    if let Some(source_knots) = source_knots.as_mut() {
                        source_knots.push(site.knot);
                    }
                    points.push(site.point.get());
                    first.push(site.first_derivative.get());
                    second.push(site.second_derivative.get());
                }
                Ok::<_, CodecError>((knots, points, first, second))
            })?;
        project(ctx, &knots, &points, &first, &second)
    }
}

/// Decode framed `a5 03 20` consolidated UV jets.
#[must_use]
#[cfg(test)]
fn a5_pcurves(data: &[u8]) -> Vec<ConsolidatedPcurve> {
    let records = consolidated_records(data);
    crate::test_support::with_service_context(|ctx| {
        crate::wire::records::family_pcurves_from_records(
            ctx,
            data,
            &records,
            ConsolidatedFamily::A,
        )
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

    fn knots(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
        ctx.collect_indexed_vec(self.sites.len(), "catia_a5_jet_knot_projection", |index| {
            Ok(self.sites[index].knot.get())
        })
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
    // The projected lanes are solver input and are released when it returns.
    let ((knots, positions, first, second), _lanes) =
        ctx.with_scoped_storage("catia_a5_limit_jet_lanes", || {
            let count = jet.sites.len();
            let mut positions = ctx.collection_vec(count, "catia A5 rolling ball positions")?;
            let mut first = ctx.collection_vec(count, "catia A5 rolling ball first jets")?;
            let mut second = ctx.collection_vec(count, "catia A5 rolling ball second jets")?;
            for sample in ctx.admit_iter(&jet.sites, "catia_a5_limit_jet_projection")? {
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
            Ok::<_, CodecError>((knots, positions, first, second))
        })?;
    let Some((knots, control_points)) = crate::nurbs::quintic_jet_bspline(
        ctx,
        A5FreeformCurve::DEGREE,
        &knots,
        &positions,
        &first,
        &second,
        cadmpeg_ir::units::FiniteVector::new,
    )?
    else {
        refusal.push_solver(
            ctx,
            format_args!(
                "consolidated_a5_03_32 rolling-ball limit curve at byte {}",
                jet.pos
            ),
            "states knot-aligned jet samples the degree-5 B-spline lowering does not close",
        )?;
        return Ok(None);
    };
    let poles = ctx.collect_indexed_vec(
        control_points.len(),
        "catia_a5_limit_pole_projection",
        |index| {
            let point = control_points[index];
            Ok(Point3::new(point[0], point[1], point[2]))
        },
    )?;
    crate::nurbs::note_refusal(
        ctx,
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            ctx,
            A5FreeformCurve::DEGREE,
            knots,
            poles,
            None,
            false,
        )?,
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
    pub(in crate::families) fn knots(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
        ctx.collect_indexed_vec(
            self.sites.len(),
            "catia_a5_guide_knot_projection",
            |index| Ok(self.sites[index].knot.get()),
        )
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
    for record in ctx
        .admit_iter(records, "catia_a5_nurbs_record_scan")?
        .filter(|record| record.family() == ConsolidatedFamily::A && record.class() == 0x16)
    {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else {
            continue;
        };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(),
            payload: payload.start,
            end: end.end,
            header_token: record.header_token(),
        };
        if let Some(curve) = parse_a5_nurbs_curve(ctx, data, frame, refusal)? {
            ctx.push_vec(&mut curves, curve, "catia_a5_nurbs_curves")?;
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
    let Some((degree, knot_count, control_count, knot_start, control_start, repeated_end)) =
        (|| {
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
            at = at.checked_add(knot_count.checked_mul(8)?)?;
            if data.get(at) != Some(&0x01) {
                return None;
            }
            at += 1;
            let control_start = at;
            at = at.checked_add(control_count.checked_mul(24)?)?;
            if compact_int(data, &mut at)? != 1 || compact_int(data, &mut at)? != 2 {
                return None;
            }
            let range_origin = f64_le(data, at)?.get();
            let repeated_end = f64_le(data, at + 8)?.get();
            let scale = f64_le(data, at + 16)?.get();
            let offset = f64_le(data, at + 24)?.get();
            at += 32;
            if range_origin.to_bits() != 0.0f64.to_bits()
                || scale.to_bits() != 1.0f64.to_bits()
                || offset.to_bits() != 0.0f64.to_bits()
                || data.get(at..frame.end) != Some(&[0x00, 0x07])
            {
                return None;
            }
            Some((
                degree,
                knot_count,
                control_count,
                knot_start,
                control_start,
                repeated_end,
            ))
        })()
    else {
        return Ok(None);
    };
    let Some(knot_bytes) = knot_count
        .checked_mul(8)
        .and_then(|bytes| knot_start.checked_add(bytes))
        .and_then(|knot_end| data.get(knot_start..knot_end))
    else {
        return Ok(None);
    };
    let Some(control_bytes) = control_count
        .checked_mul(24)
        .and_then(|bytes| control_start.checked_add(bytes))
        .and_then(|control_end| data.get(control_start..control_end))
    else {
        return Ok(None);
    };
    let mut previous_knot = None;
    let increasing = ctx.all_by(
        knot_bytes.chunks_exact(8),
        |chunk| {
            let Some(knot) = f64_le(chunk, 0) else {
                return Ok(false);
            };
            let increasing = previous_knot.is_none_or(|previous| knot.get() > previous);
            previous_knot = Some(knot.get());
            Ok(increasing)
        },
        "catia_a5_nurbs_knot_preflight_scan",
    )?;
    if !increasing || previous_knot.is_none_or(|last| repeated_end.to_bits() != last.to_bits()) {
        return Ok(None);
    }
    if !ctx.all_by(
        control_bytes.chunks_exact(24),
        |chunk| Ok(f64_point(chunk, 0).is_some()),
        "catia_a5_nurbs_control_preflight_scan",
    )? {
        return Ok(None);
    }
    let control_points =
        ctx.collect_indexed_vec(control_count, "catia_a5_nurbs_control_points", |index| {
            f64_point(control_bytes, index * 24)
                .ok_or_else(|| CodecError::malformed("CATIA A5 NURBS control point changed"))
        })?;
    let Some(expanded_count) = usize::try_from(degree)
        .ok()
        .and_then(|degree| control_count.checked_add(degree))
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    // The first and last distinct knots are clamped with multiplicity six;
    // every interior knot has multiplicity three.
    let mut knots = Vec::new();
    ctx.reserve_capacity(&mut knots, expanded_count, "catia_a5_nurbs_expanded_knots")?;
    for index in ctx.admit_iter(0..knot_count, "catia_a5_nurbs_knot_expansion_scan")? {
        let Some(knot) = f64_le(knot_bytes, index * 8) else {
            return Err(CodecError::malformed("CATIA A5 NURBS knot changed"));
        };
        let multiplicity = if index == 0 || index + 1 == knot_count {
            6
        } else {
            3
        };
        let length = knots.len() + multiplicity;
        ctx.resize_vec(
            &mut knots,
            length,
            knot.get(),
            "catia_a5_nurbs_expanded_knots",
        )?;
    }
    crate::nurbs::note_refusal(
        ctx,
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            ctx,
            degree,
            knots,
            control_points,
            None,
            false,
        )?,
        refusal,
        format_args!("a5 NURBS curve record at byte {}", frame.pos),
    )
    .map(|geometry| {
        geometry.map(|geometry| A5NurbsCurve {
            pos: frame.pos,
            header_token: frame.header_token,
            geometry,
        })
    })
}

/// Decode `a5/a6/a7 03 39` guide-curve and unit-direction jets.
#[cfg(test)]
fn a5_guide_curves(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<Vec<A5GuideCurve>, CodecError> {
    let records = consolidated_records(data);
    a5_guide_curves_from_records(ctx, data, &records)
}

pub(in crate::families) fn a5_guide_curves_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<A5GuideCurve>, CodecError> {
    let mut curves = Vec::new();
    for record in ctx
        .admit_iter(records, "catia_a5_guide_record_scan")?
        .filter(|record| record.family() == ConsolidatedFamily::A && record.class() == 0x39)
    {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else {
            continue;
        };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(),
            payload: payload.start,
            end: end.end,
            header_token: record.header_token(),
        };
        if let Some(curve) = parse_a5_guide_curve(ctx, data, frame)? {
            ctx.push_vec(&mut curves, curve, "catia_a5_guide_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a5_guide_curve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: ConsolidatedFrame,
) -> Result<Option<A5GuideCurve>, CodecError> {
    let mut at = frame.payload;
    let Some(count) = compact_int(data, &mut at).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    let Some(degree) = compact_int(data, &mut at) else {
        return Ok(None);
    };
    if compact_int(data, &mut at).and_then(|value| usize::try_from(value).ok()) != Some(count)
        || count < 2
        || !(1..=9).contains(&degree)
    {
        return Ok(None);
    }
    let Some(next) = consume_array_marker(data, at) else {
        return Ok(None);
    };
    at = next;
    let Some(block_bytes) = count.checked_mul(48) else {
        return Ok(None);
    };
    let Some(known_bytes) = count
        .checked_mul(8)
        .and_then(|bytes| {
            block_bytes
                .checked_mul(3)
                .and_then(|blocks| bytes.checked_add(blocks))
        })
        .and_then(|bytes| bytes.checked_add(48))
    else {
        return Ok(None);
    };
    if at
        .checked_add(known_bytes)
        .is_none_or(|last| last > frame.end)
    {
        return Ok(None);
    }
    let knot_start = at;
    let Some(first_block) = at.checked_add(count * 8) else {
        return Ok(None);
    };
    let Some(second_block) = first_block.checked_add(block_bytes) else {
        return Ok(None);
    };
    let Some(third_block) = second_block.checked_add(block_bytes) else {
        return Ok(None);
    };
    let Some(third_end) = third_block.checked_add(block_bytes) else {
        return Err(ctx.refuse_codec_limit("catia_a5_guide_materialization", u64::MAX, u64::MAX));
    };
    if third_end.checked_add(48) != Some(frame.end) {
        return Ok(None);
    }
    let mut previous_knot = None;
    let sites = retain_built(ctx, "catia_a5_guide_sites", || {
        let mut sites = ctx.collection_vec(count, "catia_a5_guide_sites")?;
        let mut steps = 0..count;
        while let Some(index) = ctx.next_charged(&mut steps, "catia_a5_guide_materialization")? {
            let (Some(value), Some(first_derivative), Some(second_derivative), Some(knot)) = (
                read_f64_array::<6>(data, first_block + index * 48),
                read_f64_array::<6>(data, second_block + index * 48),
                read_f64_array::<6>(data, third_block + index * 48),
                f64_le(data, knot_start + index * 8),
            ) else {
                return Ok(None);
            };
            if previous_knot.is_some_and(|previous| previous >= knot.get()) {
                return Ok(None);
            }
            previous_knot = Some(knot.get());
            let direction = [
                value[3].get() - value[0].get(),
                value[4].get() - value[1].get(),
                value[5].get() - value[2].get(),
            ];
            let length =
                (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
            if (length - 1.0).abs().partial_cmp(&EPS_GUIDE_DIRECTION_UNIT)
                != Some(std::cmp::Ordering::Less)
            {
                return Ok(None);
            }
            sites.push(GuideCurveSite {
                knot,
                first_derivative: first_derivative.into(),
                second_derivative: second_derivative.into(),
                point: [value[0], value[1], value[2]].into(),
            });
        }
        Ok(Some(sites))
    })?;
    let Some(sites) = sites else {
        return Ok(None);
    };
    Ok(Some(A5GuideCurve {
        pos: frame.pos,
        header_token: frame.header_token,
        degree,
        sites,
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
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<u32>, CodecError> {
        ctx.collect_indexed_vec(self.sites.len(), "catia_a8_jet_multiplicities", |index| {
            Ok(self.sites[index].multiplicity)
        })
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
    let stations = ctx.collect_indexed_vec(jet.sites.len(), "catia_a8_jet_stations", |index| {
        let sample = &jet.sites[index];
        Ok(cadmpeg_ir::geometry::RollingBallJetStation {
            knot: sample.knot,
            multiplicity: sample.multiplicity,
            site: rolling_ball_jet_site(
                &sample.site,
                sample.first_derivatives,
                sample.second_derivatives,
            ),
        })
    })?;
    Ok(cadmpeg_ir::geometry::RollingBallJetStations::from_parts(
        A8FreeformCurve::DEGREE,
        stations,
        ctx,
    )?
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
pub(in crate::families) fn a8_freeform_curves(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<A8FreeformCurve>, CodecError> {
    let mut curves = Vec::new();
    for frame in a8_frames(ctx, data, 0x32)? {
        if let Some(curve) = parse_a8_curve(ctx, data, frame)? {
            ctx.push_vec(&mut curves, curve, "catia_a8_freeform_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a8_curve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: A8Frame,
) -> Result<Option<A8FreeformCurve>, CodecError> {
    let A8Frame {
        pos,
        payload,
        end,
        object_id,
    } = frame;
    let Some((count, knot_start, multiplicity_start, block_bytes)) = (|| {
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
        at = at.checked_add(count.checked_mul(8)?)?;
        Some((count, knot_start, at, block_bytes))
    })() else {
        return Ok(None);
    };
    let mut at = multiplicity_start;
    let mut steps = 0..count;
    while let Some(index) =
        ctx.next_charged(&mut steps, "catia_a8_jet_multiplicity_preflight_scan")?
    {
        let Some(multiplicity) = compact_int(data, &mut at) else {
            return Ok(None);
        };
        if (index == 0 || index + 1 == count) && multiplicity != 6 {
            return Ok(None);
        }
        if index != 0 && index + 1 != count && !matches!(multiplicity, 1 | 3) {
            return Ok(None);
        }
    }
    let Some(block_start) = (|| {
        let blocks_end = at.checked_add(block_bytes.checked_mul(3)?)?;
        if blocks_end > end || end - blocks_end != 59 {
            return None;
        }
        Some(at)
    })() else {
        return Ok(None);
    };
    let second_block = block_start + block_bytes;
    let third_block = second_block + block_bytes;
    let mut previous_knot = None;
    let mut multiplicity_at = multiplicity_start;
    let sites = retain_built(ctx, "catia_a8_freeform_sites", || {
        let mut sites = ctx.collection_vec(count, "catia_a8_freeform_sites")?;
        let mut steps = 0..count;
        while let Some(index) = ctx.next_charged(&mut steps, "catia_a8_jet_materialization")? {
            let (
                Some(knot),
                Some(multiplicity),
                Some(positions),
                Some(first_derivatives),
                Some(second_derivatives),
            ) = (
                f64_le(data, knot_start + index * 8),
                compact_int(data, &mut multiplicity_at),
                read_f64_array::<10>(data, block_start + index * 80),
                read_f64_array::<10>(data, second_block + index * 80),
                read_f64_array::<10>(data, third_block + index * 80),
            )
            else {
                return Ok(None);
            };
            if previous_knot.is_some_and(|previous| knot.get() <= previous) {
                return Ok(None);
            }
            previous_knot = Some(knot.get());
            let Some(site) = rolling_ball_site(positions) else {
                return Ok(None);
            };
            sites.push(A8FreeformJet {
                knot,
                multiplicity,
                site,
                first_derivatives,
                second_derivatives,
            });
        }
        Ok(Some(sites))
    })?;
    let Some(sites) = sites else {
        return Ok(None);
    };
    Ok(Some(A8FreeformCurve {
        pos,
        object_id,
        sites,
    }))
}

/// Decode framed `a5 03 32` rolling-ball jet records.
#[cfg(test)]
fn a5_freeform_curves(
    ctx: &DecodeContext<'_>,
    data: &[u8],
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
    for record in ctx
        .admit_iter(records, "catia_a5_jet_record_scan")?
        .filter(|record| record.family() == ConsolidatedFamily::A && record.class() == 0x32)
    {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else {
            continue;
        };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(),
            payload: payload.start,
            end: end.end,
            header_token: record.header_token(),
        };
        if let Some(curve) = parse_a5_curve(ctx, data, frame)? {
            ctx.push_vec(&mut curves, curve, "catia_a5_freeform_curves")?;
        }
    }
    Ok(curves)
}

fn parse_a5_curve(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: ConsolidatedFrame,
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
        at = at.checked_add(count.checked_mul(8)?)?;
        Some((count, knot_start, at, block_bytes))
    })() else {
        return Ok(None);
    };
    let second_block = block_start + block_bytes;
    let third_block = second_block + block_bytes;
    let mut previous_knot = None;
    let sites = retain_built(ctx, "catia_a5_freeform_sites", || {
        let mut sites = ctx.collection_vec(count, "catia_a5_freeform_sites")?;
        let mut steps = 0..count;
        while let Some(index) = ctx.next_charged(&mut steps, "catia_a5_jet_materialization")? {
            let (Some(knot), Some(positions), Some(first_derivatives), Some(second_derivatives)) = (
                f64_le(data, knot_start + index * 8),
                read_f64_array::<10>(data, block_start + index * 80),
                read_f64_array::<10>(data, second_block + index * 80),
                read_f64_array::<10>(data, third_block + index * 80),
            ) else {
                return Ok(None);
            };
            if previous_knot.is_some_and(|previous| knot.get() <= previous) {
                return Ok(None);
            }
            previous_knot = Some(knot.get());
            let Some(site) = rolling_ball_site(positions) else {
                return Ok(None);
            };
            sites.push(A5FreeformJet {
                knot,
                site,
                first_derivatives,
                second_derivatives,
            });
        }
        Ok(Some(sites))
    })?;
    let Some(sites) = sites else {
        return Ok(None);
    };
    Ok(Some(A5FreeformCurve {
        pos,
        header_token,
        sites,
    }))
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
#[cfg(test)]
fn a8_pcurves(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<Vec<A8Pcurve>, CodecError> {
    let mut pcurves = Vec::new();
    for frame in
        object_stream_frames(ctx, data)?.filter(|frame| frame.class == 0x20 && frame.family == 0xa8)
    {
        if let Some(pcurve) =
            parse_object_stream_pcurve(ctx, data, frame.payload, frame.end, frame.object_id)?
        {
            ctx.push_vec(&mut pcurves, pcurve, "catia_a8_pcurves")?;
        }
    }
    Ok(pcurves)
}

/// Decode framed `a8 <flag> 20` and `b5 <flag> 20` object-stream UV jet records.
pub(in crate::families) fn object_stream_pcurves(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<A8Pcurve>, CodecError> {
    let mut pcurves = Vec::new();
    for frame in object_stream_frames(ctx, data)?.filter(|frame| frame.class == 0x20) {
        if let Some(pcurve) =
            parse_object_stream_pcurve(ctx, data, frame.payload, frame.end, frame.object_id)?
        {
            ctx.push_vec(&mut pcurves, pcurve, "catia_object_stream_pcurves")?;
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
    let Some((support_id, count, knot_start, array_bytes, mut at)) = (|| {
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
        let knot_start = at;
        at = at.checked_add(knot_bytes)?;
        Some((support_id, count, knot_start, array_bytes, at))
    })() else {
        return Ok(None);
    };
    let mut steps = 0..count;
    while let Some(index) = ctx.next_charged(
        &mut steps,
        "catia_object_stream_pcurve_multiplicity_preflight_scan",
    )? {
        let Some(multiplicity) = compact_int(data, &mut at) else {
            return Ok(None);
        };
        if (index == 0 || index + 1 == count) && multiplicity != 6 {
            return Ok(None);
        }
        if index != 0 && index + 1 != count && multiplicity != 3 {
            return Ok(None);
        }
    }
    let Some((mode, array_starts, range)) = (|| {
        if usize::try_from(compact_int(data, &mut at)?).ok()? != count {
            return None;
        }
        let mode = *data.get(at)?;
        at += 1;
        if at.checked_add(array_bytes.checked_add(18)?)? > end {
            return None;
        }
        let u = at;
        let knot_bytes = count.checked_mul(8)?;
        at = at.checked_add(knot_bytes)?;
        let v = at;
        at = at.checked_add(knot_bytes)?;
        let du = at;
        at = at.checked_add(knot_bytes)?;
        let dv = at;
        at = at.checked_add(knot_bytes)?;
        if data.get(at) != Some(&0x05) {
            return None;
        }
        at += 1;
        let ddu = at;
        at = at.checked_add(knot_bytes)?;
        let ddv = at;
        at = at.checked_add(knot_bytes)?;
        let range = [f64_le(data, at)?, f64_le(data, at + 8)?];
        at += 16;
        if data.get(at) != Some(&0x07) || mode % 4 != 1 || range[0] >= range[1] || end != at + 1 {
            return None;
        }
        Some((mode, [u, v, du, dv, ddu, ddv], range))
    })() else {
        return Ok(None);
    };
    let parsed = (support_id, mode, count, knot_start, array_starts, range);
    #[cfg(test)]
    let (support_id, mode, count, knot_start, array_starts, range) = parsed;
    #[cfg(not(test))]
    let (support_id, _, count, knot_start, array_starts, range) = parsed;
    let [u, v, du, dv, ddu, ddv] = array_starts;
    let mut previous_knot = None;
    let sites = retain_built(ctx, "catia_object_stream_pcurve_sites", || {
        let mut sites = ctx.collection_vec(count, "catia_object_stream_pcurve_sites")?;
        let mut steps = 0..count;
        while let Some(index) =
            ctx.next_charged(&mut steps, "catia_object_stream_pcurve_materialization")?
        {
            let offset = index * 8;
            let (Some(knot), Some(u), Some(v), Some(du), Some(dv), Some(ddu), Some(ddv)) = (
                f64_le(data, knot_start + offset),
                f64_le(data, u + offset),
                f64_le(data, v + offset),
                f64_le(data, du + offset),
                f64_le(data, dv + offset),
                f64_le(data, ddu + offset),
                f64_le(data, ddv + offset),
            ) else {
                return Ok(None);
            };
            if previous_knot.is_some_and(|previous| knot <= previous) {
                return Ok(None);
            }
            previous_knot = Some(knot);
            sites.push(A8PcurveSite {
                knot,
                point: [u, v].into(),
                first_derivative: [du, dv].into(),
                second_derivative: [ddu, ddv].into(),
            });
        }
        Ok(Some(sites))
    })?;
    let Some(sites) = sites else {
        return Ok(None);
    };
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
    for frame in a8_frames(ctx, data, 0x34)? {
        let Some(parsed) = parse_a8_surface_header(ctx, data, frame)? else {
            continue;
        };
        if let Some(surface) = a8_surface_from_parsed(ctx, data, parsed, refusal)? {
            ctx.push_vec(&mut surfaces, surface, "catia_a8_inline_surfaces")?;
        }
    }
    Ok(surfaces)
}

/// Decode every complete common-form object-stream NURBS surface, including
/// parameter records whose pole grids occupy a uniquely bounded external
/// allocation.
pub(in crate::families) fn resolved_a8_surfaces(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let mut grids = A8ExternalGridSites::default();
    let mut surfaces = Vec::new();
    for frame in a8_frames(ctx, data, 0x34)? {
        if let Some(surface) = resolved_a8_surface_from_object_frame(
            ctx,
            data,
            frame.pos,
            frame.end,
            frame.object_id,
            &mut grids,
            refusal,
        )? {
            ctx.push_vec(&mut surfaces, surface, "catia_a8_resolved_surfaces")?;
        }
    }
    Ok(surfaces)
}

/// Decode every structurally complete `a8 <flag> 34` parameter lattice, including
/// records whose pole representation is not inline.
#[cfg(test)]
fn a8_surface_headers<'a>(
    ctx: &'a DecodeContext<'_>,
    data: &'a [u8],
) -> Result<impl Iterator<Item = Result<A8SurfaceHeader, CodecError>> + 'a, CodecError> {
    Ok(a8_frames(ctx, data, 0x34)?.filter_map(
        move |frame| match a8_surface_header_from_object_frame(
            ctx,
            data,
            frame.pos,
            frame.end,
            frame.object_id,
        ) {
            Ok(Some(header)) => Some(Ok(header)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        },
    ))
}

/// Decode one selected `a8 <flag> 34` frame's parameter lattice.
pub(in crate::families) fn a8_surface_header_from_object_frame(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
) -> Result<Option<A8SurfaceHeader>, CodecError> {
    Ok(
        parse_selected_a8_surface_header(ctx, data, start, end, object_id)?
            .map(|parsed| parsed.header),
    )
}

/// Decode one selected `a8 <flag> 34` frame and its complete pole grid.
///
/// `grids` indexes the external pole allocations of `data`; one index serves
/// every frame of the same byte stream.
pub(in crate::families) fn resolved_a8_surface_from_object_frame<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    object_id: u32,
    grids: &mut A8ExternalGridSites<'ctx>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FreeformSurface>, CodecError> {
    let Some(parsed) = parse_selected_a8_surface_header(ctx, data, start, end, object_id)? else {
        return Ok(None);
    };
    if parsed.header.pole_storage == PoleStorage::Elided {
        a8_surface_from_external_grid(ctx, data, &parsed.header, grids, refusal)
    } else {
        a8_surface_from_parsed(ctx, data, parsed, refusal)
    }
}

/// The end of every `b5 <flag> 21` pcurve frame in one object stream, grouped
/// by the surface object id its support reference names. An external pole
/// allocation starts at such a frame end. The index is built on first use, so
/// a stream without elided-pole surfaces is not scanned for it.
#[derive(Default)]
pub(in crate::families) struct A8ExternalGridSites<'ctx> {
    by_support: Option<(BTreeMap<u32, Vec<usize>>, ScopedReservation<'ctx>)>,
}

impl<'ctx> A8ExternalGridSites<'ctx> {
    fn starts(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        data: &[u8],
        object_id: u32,
    ) -> Result<&[usize], CodecError> {
        if self.by_support.is_none() {
            self.by_support =
                Some(ctx.with_scoped_storage("catia_a8_external_grid_sites", || {
                    let mut by_support = BTreeMap::new();
                    for frame in object_stream_frames(ctx, data)? {
                        if frame.family != 0xb5 || frame.class != 0x21 {
                            continue;
                        }
                        let Some(mut at) = frame.payload.checked_add(1) else {
                            continue;
                        };
                        let Some(support) = object_stream_reference(data, &mut at) else {
                            continue;
                        };
                        ctx.push_btree_group(
                            &mut by_support,
                            support,
                            frame.end,
                            "catia_a8_external_grid_sites",
                            "catia_a8_external_grid_site_ends",
                        )?;
                    }
                    Ok::<_, CodecError>(by_support)
                })?);
        }
        let Some((by_support, _storage)) = &self.by_support else {
            return Err(CodecError::malformed("CATIA external grid index is absent"));
        };
        Ok(ctx
            .get_btree_map(by_support, &object_id, "catia_a8_external_grid_site_lookup")?
            .map_or(&[], Vec::as_slice))
    }
}

/// Resolve an elided-pole `a8 <flag> 34` carrier from its support-referenced
/// external grid allocation. The allocation occupies the complete unframed gap
/// between a length-closed `b5 <flag> 21` pcurve and the following A/B-family
/// frame; its pcurve support reference must equal the surface object id.
fn a8_surface_from_external_grid<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    data: &[u8],
    header: &A8SurfaceHeader,
    grids: &mut A8ExternalGridSites<'ctx>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FreeformSurface>, CodecError> {
    let Some(need) = ExternalGridNeed::from_header(ctx, header)? else {
        return Ok(None);
    };
    let Some(layout) = need.layout() else {
        return Ok(None);
    };
    let mut unique = None;
    let mut starts = grids.starts(ctx, data, need.object_id)?.iter();
    while let Some(&start) =
        ctx.next_charged(&mut starts, "catia_a8_external_grid_candidate_visits")?
    {
        let Some(range) = external_grid_candidate(ctx, data, layout, start)? else {
            continue;
        };
        if unique.replace(range).is_some() {
            return Ok(None);
        }
    }
    let Some(range) = unique else {
        return Ok(None);
    };
    let Some((control_points, weights)) = retain_built(ctx, "catia_a8_external_grid", || {
        let Some(control_points) = read_point_rows(
            ctx,
            data,
            range.start,
            layout.rows,
            layout.columns,
            "catia_a8_external_poles",
        )?
        else {
            return Ok(None);
        };
        let weights = if layout.rational {
            let Some(weights) = read_weight_rows(
                ctx,
                data,
                range.start + layout.pole_bytes,
                layout.rows,
                layout.columns,
                "catia_a8_external_weights",
            )?
            else {
                return Ok(None);
            };
            Some(weights)
        } else {
            None
        };
        Ok(Some((control_points, weights)))
    })?
    else {
        return Ok(None);
    };
    let u_knots = header.u_knots.expanded(ctx)?;
    let v_knots = header.v_knots.expanded(ctx)?;
    crate::nurbs::note_refusal(
        ctx,
        NurbsSurface::from_checked_lanes(
            ctx,
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(header.u_degree, u_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(header.v_degree, v_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, weights),
            false,
        )?,
        refusal,
        format_args!(
            "a8 NURBS surface record #{} at byte {}",
            header.object_id, header.pos
        ),
    )
    .map(|geometry| {
        geometry.map(|geometry| FreeformSurface {
            pos: header.pos,
            identity: Some(header.object_id),
            geometry,
        })
    })
}

/// Return every complete support-bound external A8 pole allocation.
pub(in crate::families) fn a8_external_grid_ranges(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<Range<usize>>, CodecError> {
    let mut grids = A8ExternalGridSites::default();
    let mut ranges = Vec::new();
    for frame in a8_frames(ctx, data, 0x34)? {
        let Some(layout) = scan_a8_surface_layout(ctx, data, frame)? else {
            continue;
        };
        let Some(grid) = ExternalGridNeed::from_layout(frame.object_id, &layout).layout() else {
            continue;
        };
        for &start in ctx.admit_iter(
            grids.starts(ctx, data, frame.object_id)?,
            "catia_a8_external_grid_candidate_visits",
        )? {
            if let Some(range) = external_grid_candidate(ctx, data, grid, start)? {
                ctx.push_vec(&mut ranges, range, "catia_a8_external_grid_ranges")?;
            }
        }
    }
    ctx.sort_unstable_by_key(
        &mut ranges,
        |value| (value.start, value.end),
        Ord::cmp,
        "catia_a8_external_grid_ranges_sort",
    )?;
    ctx.dedup_by(
        &mut ranges,
        |left, right| Ok(left == right),
        "catia_a8_external_grid_ranges_dedup",
    )?;
    Ok(ranges)
}

#[derive(Clone, Copy)]
struct ExternalGridNeed {
    object_id: u32,
    u_count: u32,
    v_count: u32,
    rational: bool,
    pole_storage: PoleStorage,
}

/// Byte layout of one external pole allocation.
#[derive(Clone, Copy)]
struct ExternalGridLayout {
    rows: usize,
    columns: usize,
    pole_bytes: usize,
    grid_bytes: usize,
    rational: bool,
}

impl ExternalGridNeed {
    fn from_header(
        ctx: &DecodeContext<'_>,
        header: &A8SurfaceHeader,
    ) -> Result<Option<Self>, CodecError> {
        let (Some(u_count), Some(v_count)) = (header.u_count(ctx)?, header.v_count(ctx)?) else {
            return Ok(None);
        };
        Ok(Some(Self {
            object_id: header.object_id,
            u_count,
            v_count,
            rational: header.rational,
            pole_storage: header.pole_storage,
        }))
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

    /// The allocation layout of an elided-pole surface; inline surfaces have none.
    fn layout(self) -> Option<ExternalGridLayout> {
        if self.pole_storage != PoleStorage::Elided {
            return None;
        }
        let rows = usize::try_from(self.u_count).ok()?;
        let columns = usize::try_from(self.v_count).ok()?;
        let poles = crate::nurbs_surface_control_count(rows, columns)?;
        let pole_bytes = poles.checked_mul(24)?;
        let weight_bytes = if self.rational {
            poles.checked_mul(8)?
        } else {
            0
        };
        Some(ExternalGridLayout {
            rows,
            columns,
            pole_bytes,
            grid_bytes: pole_bytes.checked_add(weight_bytes)?,
            rational: self.rational,
        })
    }
}

/// Return the allocation starting at `start` when it holds exactly the finite
/// poles and nonzero weights the layout needs and an object frame follows it.
fn external_grid_candidate(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    layout: ExternalGridLayout,
    start: usize,
) -> Result<Option<Range<usize>>, CodecError> {
    let Some(end) = start.checked_add(layout.grid_bytes) else {
        return Ok(None);
    };
    if object_stream_frame(data, end).is_none() {
        return Ok(None);
    }
    let Some(grid) = data.get(start..end) else {
        return Ok(None);
    };
    let (poles, weights) = grid.split_at(layout.pole_bytes);
    if !ctx.all_by(
        poles.chunks_exact(24),
        |chunk| Ok(f64_point(chunk, 0).is_some()),
        "catia_a8_external_grid_candidate_scan",
    )? {
        return Ok(None);
    }
    if !ctx.all_by(
        weights.chunks_exact(8),
        |chunk| Ok(nonzero_weight(chunk, 0).is_some()),
        "catia_a8_external_grid_weight_scan",
    )? {
        return Ok(None);
    }
    Ok(Some(start..end))
}

fn nonzero_weight(data: &[u8], at: usize) -> Option<NonZeroReal> {
    NonZeroReal::new(f64_le(data, at)?.get())
}

/// Read `rows` rows of `columns` finite points from consecutive 24-byte records.
fn read_point_rows(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    rows: usize,
    columns: usize,
    operation: &'static str,
) -> Result<Option<Vec<Vec<FinitePoint3>>>, CodecError> {
    let Some(lane) = data.get(start..) else {
        return Ok(None);
    };
    read_rows(ctx, lane, rows, columns, 24, f64_point, operation)
}

/// Read `rows` rows of `columns` nonzero weights from consecutive scalars.
fn read_weight_rows(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    rows: usize,
    columns: usize,
    operation: &'static str,
) -> Result<Option<Vec<Vec<NonZeroReal>>>, CodecError> {
    let Some(lane) = data.get(start..) else {
        return Ok(None);
    };
    read_rows(ctx, lane, rows, columns, 8, nonzero_weight, operation)
}

fn read_rows<T>(
    ctx: &DecodeContext<'_>,
    lane: &[u8],
    rows: usize,
    columns: usize,
    width: usize,
    read: impl Fn(&[u8], usize) -> Option<T>,
    operation: &'static str,
) -> Result<Option<Vec<Vec<T>>>, CodecError> {
    let mut grid = ctx.collection_vec(rows, operation)?;
    let mut at = 0usize;
    let mut steps = 0..rows;
    while ctx.next_charged(&mut steps, operation)?.is_some() {
        let mut row = ctx.collection_vec(columns, operation)?;
        let mut steps = 0..columns;
        while ctx.next_charged(&mut steps, operation)?.is_some() {
            let Some(value) = read(lane, at) else {
                return Ok(None);
            };
            row.push(value);
            at = at
                .checked_add(width)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        }
        grid.push(row);
    }
    Ok(Some(grid))
}

/// Decode consolidated `a5 03 34` NURBS surface carriers.  This family uses
/// implicit clamped multiplicities instead of the explicit `a8` vectors.
pub(crate) fn a5_surfaces(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let records = crate::wire::records::consolidated_records_in_sources(
        ctx,
        data,
        std::iter::once(std::iter::once(crate::wire::records::SourceExtent::whole(
            data,
        ))),
    )?;
    a5_surfaces_from_records(ctx, data, &records, refusal)
}

pub(in crate::families) fn a5_surfaces_from_records(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurface>, CodecError> {
    let mut surfaces = Vec::new();
    for record in ctx
        .admit_iter(records, "catia_a5_surface_record_scan")?
        .filter(|record| record.family() == ConsolidatedFamily::A && record.class() == 0x34)
    {
        let (Some(payload), Some(end)) = (record.payload(), record.range()) else {
            continue;
        };
        let frame = ConsolidatedFrame {
            pos: record.byte_offset(),
            payload: payload.start,
            end: end.end,
            header_token: record.header_token(),
        };
        if let Some(surface) = a5_surface(ctx, data, frame, refusal)? {
            ctx.push_vec(&mut surfaces, surface, "catia_a5_surfaces")?;
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
    let ConsolidatedFrame {
        pos, payload, end, ..
    } = frame;
    let mut at = payload;
    let Some(u_degree) = data.get(at).and_then(|&byte| a5_int(byte)) else {
        return Ok(None);
    };
    at += 1;
    let Some(u_distinct_count) = data
        .get(at)
        .and_then(|&byte| a5_int(byte))
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(next) = a5_array_marker(data, at + 1) else {
        return Ok(None);
    };
    at = next;
    let Some(u_distinct) = a5_distinct_values(ctx, data, &mut at, u_distinct_count, end)? else {
        return Ok(None);
    };
    let Some(v_degree) = data.get(at).and_then(|&byte| a5_int(byte)) else {
        return Ok(None);
    };
    at += 1;
    let Some(v_distinct_count) = data
        .get(at)
        .and_then(|&byte| a5_int(byte))
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(next) = a5_array_marker(data, at + 1) else {
        return Ok(None);
    };
    at = next;
    let Some(v_distinct) = a5_distinct_values(ctx, data, &mut at, v_distinct_count, end)? else {
        return Ok(None);
    };
    let Some(&mode) = data.get(at) else {
        return Ok(None);
    };
    at += 1;
    if !knots_strictly_increasing(&u_distinct, |count| {
        ctx.charge_work(count, "catia_a5_surface_knot_order_scan")
    })? || !knots_strictly_increasing(&v_distinct, |count| {
        ctx.charge_work(count, "catia_a5_surface_knot_order_scan")
    })? {
        return Ok(None);
    }
    let Some((u_knots, u_count)) = a5_knots(ctx, &u_distinct, u_degree)? else {
        return Ok(None);
    };
    let Some((v_knots, v_count)) = a5_knots(ctx, &v_distinct, v_degree)? else {
        return Ok(None);
    };
    let (Some(u_count), Some(v_count)) =
        (usize::try_from(u_count).ok(), usize::try_from(v_count).ok())
    else {
        return Ok(None);
    };
    if !matches!(mode, 0x01 | 0x05) {
        return Ok(None);
    }
    let Some(end_poles) = crate::nurbs_surface_control_count(u_count, v_count)
        .and_then(|poles| poles.checked_mul(24))
        .and_then(|bytes| at.checked_add(bytes))
        .filter(|&end_poles| end_poles <= end)
    else {
        return Ok(None);
    };
    let Some((control_points, weights)) = retain_built(ctx, "catia_a5_surface_grid", || {
        let Some(control_points) =
            read_point_rows(ctx, data, at, u_count, v_count, "catia_a5_surface_poles")?
        else {
            return Ok(None);
        };
        let mut at = end_poles;
        let weights = if mode == 0x05 {
            let Some(weights) = a5_weights(ctx, data, &mut at, u_count, v_count, end)? else {
                return Ok(None);
            };
            Some(weights)
        } else {
            None
        };
        if parse_surface_tail(data, at, end).is_none() {
            return Ok(None);
        }
        Ok(Some((control_points, weights)))
    })?
    else {
        return Ok(None);
    };
    crate::nurbs::note_refusal(
        ctx,
        NurbsSurface::from_checked_lanes(
            ctx,
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, weights),
            false,
        )?,
        refusal,
        format_args!("a5 NURBS surface record at byte {pos}"),
    )
    .map(|geometry| {
        geometry.map(|geometry| FreeformSurface {
            pos,
            identity: None,
            geometry,
        })
    })
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
    let Some(frame) = object_stream_frame(data, start) else {
        return Ok(None);
    };
    if !(frame.family == 0xa8
        && frame.class == 0x34
        && frame.end == end
        && frame.object_id == object_id)
    {
        return Ok(None);
    }
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
    ctx: &DecodeContext<'_>,
    data: &[u8],
    at: &mut usize,
    count: usize,
    degree: u32,
    end: usize,
) -> Result<Option<(A8LaneLayout, f64, f64)>, CodecError> {
    let distinct_start = *at;
    let Some(distinct_end) = count
        .checked_mul(8)
        .and_then(|bytes| distinct_start.checked_add(bytes))
        .filter(|&distinct_end| distinct_end <= end)
    else {
        return Ok(None);
    };
    let Some(bytes) = data.get(distinct_start..distinct_end) else {
        return Ok(None);
    };
    let mut first = None;
    let mut last = None;
    let increasing = ctx.all_by(
        bytes.chunks_exact(8),
        |chunk| {
            let Some(value) = f64_le(chunk, 0) else {
                return Ok(false);
            };
            let value = value.get();
            if last.is_some_and(|previous| previous >= value) {
                return Ok(false);
            }
            first.get_or_insert(value);
            last = Some(value);
            Ok(true)
        },
        "catia_a8_distinct_knot_preflight_scan",
    )?;
    if !increasing {
        return Ok(None);
    }
    *at = distinct_end;
    let multiplicity_start = *at;
    let mut total = 0u32;
    let mut steps = 0..count;
    while ctx
        .next_charged(&mut steps, "catia_a8_surface_multiplicity_preflight_scan")?
        .is_some()
    {
        let Some(multiplicity) = compact_int(data, at) else {
            return Ok(None);
        };
        if multiplicity == 0 {
            return Ok(None);
        }
        let Some(next_total) = total.checked_add(multiplicity) else {
            return Err(ctx.refuse_codec_limit(
                "catia_a8_distinct_knot_preflight_scan",
                u64::MAX,
                u64::MAX,
            ));
        };
        total = next_total;
    }
    let Some(degree_width) = degree.checked_add(1) else {
        return Err(ctx.refuse_codec_limit(
            "catia_a8_distinct_knot_preflight_scan",
            u64::MAX,
            u64::MAX,
        ));
    };
    let Some(poles) = total.checked_sub(degree_width) else {
        return Ok(None);
    };
    let (Some(first), Some(last)) = (first, last) else {
        return Ok(None);
    };
    Ok(Some((
        A8LaneLayout {
            distinct_start,
            multiplicity_start,
            count,
            poles,
        },
        first,
        last,
    )))
}

fn scan_a8_surface_layout(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: A8Frame,
) -> Result<Option<A8SurfaceLayout>, CodecError> {
    let A8Frame { payload, end, .. } = frame;
    if end.checked_sub(payload).is_none_or(|length| length < 20) {
        return Ok(None);
    }
    let Some(mut at) = payload.checked_add(1) else {
        return Err(ctx.refuse_codec_limit("catia_a8_lane_preflight", u64::MAX, u64::MAX));
    };
    let Some(u_degree) = compact_int(data, &mut at) else {
        return Ok(None);
    };
    let Some(next_at) = at.checked_add(2) else {
        return Err(ctx.refuse_codec_limit("catia_a8_lane_preflight", u64::MAX, u64::MAX));
    };
    at = next_at;
    let Some(u_count) = compact_int(data, &mut at).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(next_at) = consume_array_marker(data, at) else {
        return Ok(None);
    };
    at = next_at;
    let Some((u, _, _)) = scan_a8_lane(ctx, data, &mut at, u_count, u_degree, end)? else {
        return Ok(None);
    };
    let Some(v_degree) = compact_int(data, &mut at) else {
        return Ok(None);
    };
    let Some(next_at) = at.checked_add(2) else {
        return Err(ctx.refuse_codec_limit("catia_a8_lane_preflight", u64::MAX, u64::MAX));
    };
    at = next_at;
    let Some(v_count) = compact_int(data, &mut at).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(next_at) = consume_array_marker(data, at) else {
        return Ok(None);
    };
    at = next_at;
    let Some((v, v_first, v_last)) = scan_a8_lane(ctx, data, &mut at, v_count, v_degree, end)?
    else {
        return Ok(None);
    };
    let Some(mode) = data.get(at).copied() else {
        return Ok(None);
    };
    let Some(at) = at.checked_add(1) else {
        return Err(ctx.refuse_codec_limit("catia_a8_lane_preflight", u64::MAX, u64::MAX));
    };
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || u_count < 2
        || v_count < 2
        || !matches!(mode, 0x01 | 0x05)
        || u.poles == 0
        || v.poles == 0
    {
        return Ok(None);
    }
    let v_span = v_last - v_first;
    let Some(tail_end) = at.checked_add(141) else {
        return Ok(None);
    };
    let elided = if tail_end <= end {
        closed_a8_child_run(ctx, data, tail_end, end)?
            && parse_a8_elided_surface_tail(data, at, v_span).is_some()
    } else {
        false
    };
    Ok(Some(A8SurfaceLayout {
        u_degree,
        v_degree,
        u,
        v,
        pole_start: at,
        rational: mode == 0x05,
        pole_storage: if elided {
            PoleStorage::Elided
        } else {
            PoleStorage::Inline
        },
    }))
}

fn materialize_a8_lane(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    layout: A8LaneLayout,
) -> Result<Option<A8KnotLane>, CodecError> {
    let mut distinct = Vec::new();
    ctx.reserve_vec(&mut distinct, layout.count, "catia_a8_distinct_knots")?;
    let mut at = layout.distinct_start;
    let mut steps = 0..layout.count;
    while ctx
        .next_charged(&mut steps, "catia_a8_distinct_materialization")?
        .is_some()
    {
        let Some(value) = f64_le(data, at) else {
            return Ok(None);
        };
        distinct.push(value);
        at += 8;
    }
    let mut multiplicities = Vec::new();
    ctx.reserve_vec(&mut multiplicities, layout.count, "catia_a8_multiplicities")?;
    at = layout.multiplicity_start;
    let mut steps = 0..layout.count;
    while ctx
        .next_charged(&mut steps, "catia_a8_multiplicity_materialization")?
        .is_some()
    {
        let Some(value) = compact_int(data, &mut at) else {
            return Ok(None);
        };
        multiplicities.push(value);
    }
    A8KnotLane::try_new(ctx, distinct, multiplicities)
}

fn parse_a8_surface_header(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    frame: A8Frame,
) -> Result<Option<ParsedA8SurfaceHeader>, CodecError> {
    let Some(layout) = scan_a8_surface_layout(ctx, data, frame)? else {
        return Ok(None);
    };
    let Some(u_knots) = materialize_a8_lane(ctx, data, layout.u)? else {
        return Ok(None);
    };
    let Some(v_knots) = materialize_a8_lane(ctx, data, layout.v)? else {
        return Ok(None);
    };
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
        pole_start,
        end,
    } = parsed;
    let Some(u_count) = header
        .u_count(ctx)?
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(v_count) = header
        .v_count(ctx)?
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
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
    let Some(poles) = crate::nurbs_surface_control_count(u_count, v_count) else {
        return Ok(None);
    };
    let weight_width = if rational { 32 } else { 24 };
    let Some(suffix_start) = poles
        .checked_mul(weight_width)
        .and_then(|bytes| pole_start.checked_add(bytes))
        .filter(|&suffix_start| suffix_start <= end)
    else {
        return Ok(None);
    };
    if a8_surface_suffix_start(ctx, data, suffix_start, end)?.is_none() {
        return Ok(None);
    }
    let Some((control_points, weights)) = retain_built(ctx, "catia_a8_inline_grid", || {
        let Some(control_points) = read_point_rows(
            ctx,
            data,
            pole_start,
            u_count,
            v_count,
            "catia_a8_inline_poles",
        )?
        else {
            return Ok(None);
        };
        let weights = if rational {
            let Some(weights) = read_weight_rows(
                ctx,
                data,
                pole_start + poles * 24,
                u_count,
                v_count,
                "catia_a8_inline_weights",
            )?
            else {
                return Ok(None);
            };
            Some(weights)
        } else {
            None
        };
        Ok(Some((control_points, weights)))
    })?
    else {
        return Ok(None);
    };
    let u_knots = u_knots.expanded(ctx)?;
    let v_knots = v_knots.expanded(ctx)?;
    crate::nurbs::note_refusal(
        ctx,
        NurbsSurface::from_checked_lanes(
            ctx,
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, weights),
            false,
        )?,
        refusal,
        format_args!("a8 NURBS surface record #{object_id} at byte {pos}"),
    )
    .map(|geometry| {
        geometry.map(|geometry| FreeformSurface {
            pos,
            identity: Some(object_id),
            geometry,
        })
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
    count: usize,
    end: usize,
) -> Result<Option<Vec<f64>>, CodecError> {
    if count
        .checked_mul(8)
        .and_then(|width| at.checked_add(width))
        .is_none_or(|last| last > end)
    {
        return Ok(None);
    }
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, "catia_a5_distinct_knots")?;
    let mut steps = 0..count;
    while ctx
        .next_charged(&mut steps, "catia_a5_distinct_materialization")?
        .is_some()
    {
        let Some(value) = f64_le(bytes, *at) else {
            return Ok(None);
        };
        values.push(value.get());
        *at += 8;
    }
    Ok(Some(values))
}

fn a5_knots(
    ctx: &DecodeContext<'_>,
    distinct: &[f64],
    degree: u32,
) -> Result<Option<(Vec<f64>, u32)>, CodecError> {
    // The end knots are clamped; interior knots have the fixed multiplicity
    // of the degree.
    let (endpoint, interior) = match degree {
        1 | 3 if distinct.len() >= 2 => (degree + 1, 1u32),
        5 if distinct.len() >= 2 => (6u32, 3u32),
        _ => return Ok(None),
    };
    let (endpoint, interior) = (
        usize::try_from(endpoint).map_err(|_| {
            ctx.refuse_codec_limit("catia_a5_expanded_knots", u64::MAX - 1, u64::MAX)
        })?,
        usize::try_from(interior).map_err(|_| {
            ctx.refuse_codec_limit("catia_a5_expanded_knots", u64::MAX - 1, u64::MAX)
        })?,
    );
    let Some(expanded_count) = (distinct.len() - 2)
        .checked_mul(interior)
        .and_then(|count| count.checked_add(2 * endpoint))
    else {
        return Err(ctx.refuse_codec_limit("catia_a5_expanded_knots", u64::MAX - 1, u64::MAX));
    };
    let Some(count) = expanded_count.checked_sub(endpoint) else {
        return Ok(None);
    };
    let count = u32::try_from(count)
        .map_err(|_| ctx.refuse_codec_limit("catia_a5_expanded_knots", u64::MAX - 1, u64::MAX))?;
    let mut knots = Vec::new();
    ctx.reserve_capacity(&mut knots, expanded_count, "catia_a5_expanded_knots")?;
    let last = distinct.len() - 1;
    for (index, &knot) in ctx
        .admit_iter(distinct, "catia_a5_knot_expansion_scan")?
        .enumerate()
    {
        let repeats = if index == 0 || index == last {
            endpoint
        } else {
            interior
        };
        let length = knots.len() + repeats;
        ctx.resize_vec(&mut knots, length, knot, "catia_a5_expanded_knots")?;
    }
    Ok(Some((knots, count)))
}

fn a5_weights(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
    rows: usize,
    cols: usize,
    end: usize,
) -> Result<Option<Vec<Vec<NonZeroReal>>>, CodecError> {
    if bytes.get(*at) == Some(&0x00) {
        let start = *at + 1;
        let Some(end_weights) = rows
            .checked_mul(cols)
            .and_then(|count| count.checked_mul(8))
            .and_then(|weight_bytes| start.checked_add(weight_bytes))
            .filter(|&end_weights| end_weights <= end)
        else {
            return Ok(None);
        };
        let Some(weights) =
            read_weight_rows(ctx, bytes, start, rows, cols, "catia_a5_explicit_weights")?
        else {
            return Ok(None);
        };
        *at = end_weights;
        return Ok(Some(weights));
    }
    if bytes.get(*at) != Some(&0x01) {
        return Ok(None);
    }
    // Each row either repeats the previous row or stores its first half,
    // which the second half mirrors.
    let seed_count = cols.div_ceil(2);
    let mut weights: Vec<Vec<NonZeroReal>> =
        ctx.collection_vec(rows, "catia_a5_mirrored_weights")?;
    let mut steps = 0..rows;
    while ctx
        .next_charged(&mut steps, "catia_a5_weight_row_scan")?
        .is_some()
    {
        if bytes.get(*at) == Some(&0x02) {
            *at += 1;
            let Some(previous) = weights.last() else {
                return Ok(None);
            };
            let row = ctx.copy_slice(previous, "catia_a5_weight_previous_row_copy")?;
            weights.push(row);
            continue;
        }
        if !matches!(bytes.get(*at..*at + 3), Some([0x01, 0x03 | 0x07, 0x00])) {
            return Ok(None);
        }
        *at += 3;
        let Some(end_seed) = seed_count
            .checked_mul(8)
            .and_then(|seed_bytes| at.checked_add(seed_bytes))
            .filter(|&end_seed| end_seed <= end)
        else {
            return Ok(None);
        };
        let mut row = ctx.collection_vec(cols, "catia_a5_mirrored_weights")?;
        let mut steps = 0..seed_count;
        while let Some(index) = ctx.next_charged(&mut steps, "catia_a5_weight_seed_scan")? {
            let Some(weight) = nonzero_weight(bytes, *at + index * 8) else {
                return Ok(None);
            };
            row.push(weight);
        }
        *at = end_seed;
        for offset in ctx
            .admit_iter(0..cols / 2, "catia_a5_weight_mirror_copy")?
            .rev()
        {
            row.push(row[offset]);
        }
        weights.push(row);
    }
    Ok(Some(weights))
}

#[cfg(test)]
mod tests;
