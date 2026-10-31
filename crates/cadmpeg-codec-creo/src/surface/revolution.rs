// SPDX-License-Identifier: Apache-2.0
//! Bounded cylinders with complete rotation frames.

use super::{
    InlineSurfaceBody, InlineSurfaceCarrier, PositionalCylinderFrame, SurfaceParameterRecord,
    SurfacePrototypeFamily, SurfacePrototypeRecord, SurfaceRow, EPS_INLINE_WITNESS,
};
use crate::scalar::revolution_frames;
use crate::{psb, scalar};

#[derive(Clone, Copy)]
struct Envelope {
    bounds: [[f64; 2]; 2],
    corners: [[f64; 3]; 2],
    end: usize,
    parameter_literal: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Admission {
    Admitted(PositionalCylinderFrame),
    Refused,
}

impl Admission {
    pub(super) fn frame(self) -> Option<PositionalCylinderFrame> {
        match self {
            Self::Admitted(frame) => Some(frame),
            Self::Refused => None,
        }
    }
}

fn envelope(body: &[u8], cache: &scalar::ScalarCache) -> Option<Envelope> {
    let mut values = [0.0; 10];
    let mut cursor = 0;
    let mut slot = 0usize;
    let mut parameter_literal = false;
    while slot < values.len() {
        let decoded = if slot == 0 || slot == 2 {
            revolution_frames::parameter_literal(body, cursor).map(|(value, end)| (value, 1, end))
        } else {
            None
        };
        parameter_literal |= decoded.is_some();
        let (value, count, end) = decoded.or_else(|| {
            revolution_frames::run(body, cursor, slot.checked_sub(1).map(|i| values[i]), cache)
        })?;
        let next = slot.checked_add(count)?;
        values.get_mut(slot..next)?.fill(value);
        cursor = end;
        slot = next;
    }
    if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
        cursor = psb::reference_id(body, cursor + 1).ok()?.1;
    }
    Some(Envelope {
        bounds: [[values[0], values[2]], [values[1], values[3]]],
        corners: [
            [values[4], values[5], values[6]],
            [values[7], values[8], values[9]],
        ],
        end: cursor,
        parameter_literal,
    })
}

fn angular_interval_valid(bounds: [f64; 2]) -> bool {
    let span = bounds[1] - bounds[0];
    span > 0.0 && span <= std::f64::consts::TAU + EPS_INLINE_WITNESS
}

pub(super) fn has_invalid_inline_angular_domain(body: &[u8], cache: &scalar::ScalarCache) -> bool {
    envelope(body, cache).is_some_and(|envelope| {
        envelope.parameter_literal && !angular_interval_valid(envelope.bounds[0])
    })
}

fn bounded_frame(
    envelope: Envelope,
    slots: cadmpeg_ir::units::FiniteVector<12>,
    radius: f64,
) -> Option<PositionalCylinderFrame> {
    angular_interval_valid(envelope.bounds[0]).then_some(())?;
    let m = slots.get();
    let origin = [m[9], m[10], m[11]];
    let axis = [m[2], m[5], m[8]];
    let reference = [m[0], m[3], m[6]];
    let mut frame = PositionalCylinderFrame::new(
        origin,
        axis,
        reference,
        radius,
        Some((envelope.bounds[1][1] - envelope.bounds[1][0]).abs()),
    )?;
    agrees(envelope, frame).then_some(())?;
    frame.parameter_bounds = Some(envelope.bounds);
    Some(frame)
}

fn agrees(envelope: Envelope, frame: PositionalCylinderFrame) -> bool {
    let u = frame.frame().ref_direction();
    let axis = frame.frame().axis();
    let v = crate::vecmath::cross(axis, u);
    let origin = frame.frame().origin();
    let radius = frame.radius().get();
    let [angular, axial] = envelope.bounds;
    let lower = angular[0].rem_euclid(std::f64::consts::TAU);
    let upper = lower + angular[1] - angular[0];
    let scale = envelope
        .corners
        .into_iter()
        .flatten()
        .chain(origin)
        .map(f64::abs)
        .fold(radius.max(1.0), f64::max);
    for coordinate in 0..3 {
        let at = |angle: f64| radius * (u[coordinate] * angle.cos() + v[coordinate] * angle.sin());
        let mut radial_min = at(lower).min(at(upper));
        let mut radial_max = at(lower).max(at(upper));
        let stationary = v[coordinate].atan2(u[coordinate]);
        for angle in [stationary, stationary + std::f64::consts::PI] {
            let lifted = lower + (angle - lower).rem_euclid(std::f64::consts::TAU);
            if lifted <= upper + EPS_INLINE_WITNESS {
                radial_min = radial_min.min(at(lifted));
                radial_max = radial_max.max(at(lifted));
            }
        }
        let heights = axial.map(|value| axis[coordinate] * value);
        let computed = [
            origin[coordinate] + radial_min + heights[0].min(heights[1]),
            origin[coordinate] + radial_max + heights[0].max(heights[1]),
        ];
        let stored = [
            envelope.corners[0][coordinate].min(envelope.corners[1][coordinate]),
            envelope.corners[0][coordinate].max(envelope.corners[1][coordinate]),
        ];
        if computed.into_iter().any(|value| !value.is_finite()) {
            return false;
        }
        let tolerance = EPS_INLINE_WITNESS * scale;
        let axial_coordinate = axis[coordinate].abs() >= 1.0 - super::EPS_INLINE_FRAME;
        if stored[0] < computed[0] - tolerance
            || stored[1] > computed[1] + tolerance
            || (axial_coordinate
                && computed
                    .into_iter()
                    .zip(stored)
                    .any(|(a, b)| (a - b).abs() > tolerance))
        {
            return false;
        }
    }
    true
}

pub(super) fn named_boundary(body: &[u8], cache: &scalar::ScalarCache) -> Option<usize> {
    let envelope = envelope(body, cache)?;
    body.get(envelope.end..)?
        .starts_with(b"\xe0\0srf_prim_ptr(cylinder)\0")
        .then_some(envelope.end)
}

fn inline_matrix<'a>(body: &'a [u8], cache: &scalar::ScalarCache) -> Option<&'a [u8]> {
    let envelope = envelope(body, cache)?;
    (body.get(envelope.end) == Some(&psb::token::COMPOUND_CLOSE)).then_some(())?;
    body.get(envelope.end + 1..)
}

pub(super) fn has_inline_rotation_prefix(body: &[u8], cache: &scalar::ScalarCache) -> bool {
    inline_matrix(body, cache)
        .is_some_and(|matrix| revolution_frames::cylinder_prefix(matrix, cache).is_some())
}

pub(super) fn has_inline_floating_rotation_prefix(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> bool {
    inline_matrix(body, cache)
        .is_some_and(|matrix| revolution_frames::prefix(matrix, cache).is_some())
}

pub(super) fn inline_body(body: &[u8], cache: &scalar::ScalarCache) -> Option<InlineSurfaceBody> {
    let envelope = envelope(body, cache)?;
    (body.get(envelope.end) == Some(&psb::token::COMPOUND_CLOSE)).then_some(())?;
    let start = envelope.end + 1;
    let matrix = body.get(start..)?;
    let floating_prefix = revolution_frames::prefix(matrix, cache);
    let floating = floating_prefix.is_some();
    let (slots, end) = floating_prefix
        .or_else(|| revolution_frames::cylinder_prefix(matrix, cache))?
        .ok()?;
    let (radius, close) = scalar::decode_inline_surface_suffix_scalar(body, start + end, cache)?;
    (body.get(close) == Some(&psb::token::COMPOUND_CLOSE)).then_some(())?;
    let frame = bounded_frame(envelope, slots, radius);
    if !floating && frame.is_none() {
        return None;
    }
    Some(InlineSurfaceBody {
        terminal_close: close,
        rotation_frame: true,
        matrix_frame: false,
        carrier: frame.map(|frame| InlineSurfaceCarrier::Cylinder {
            frame,
            split_bounds: None,
        }),
        rotation_cylinder: false,
    })
}

pub(super) fn named_frame(
    row: &SurfaceRow,
    parameter: &SurfaceParameterRecord,
    rows: &[SurfaceRow],
    prototypes: &[SurfacePrototypeRecord],
    bounds: &[(usize, usize)],
    cache: &scalar::ScalarCache,
) -> Option<(Admission, bool)> {
    let envelope = envelope(&parameter.body, cache)?;
    (envelope.end == parameter.body.len()).then_some(())?;
    let (frame_start, frame_end) = crate::decode::uniqueness::exactly_one(
        bounds
            .iter()
            .copied()
            .filter(|(start, end)| row.offset >= *start && row.offset < *end),
    )?;
    let next_row = rows
        .iter()
        .filter(|next| next.offset > row.offset && next.offset < frame_end)
        .map(|next| next.offset)
        .min()
        .unwrap_or(frame_end);
    let prototype = crate::decode::uniqueness::exactly_one(prototypes.iter().filter(|p| {
        p.family == SurfacePrototypeFamily::Cylinder
            && p.offset > row.offset
            && p.offset < next_row
            && p.offset >= frame_start
    }))?;
    let field = prototype.field("local_sys")?;
    let body = field.body.strip_prefix(&[psb::token::SCALAR_BODY, 4, 3])?;
    let floating = revolution_frames::prefix(body, cache).is_some();
    let decoded = revolution_frames::cylinder_prefix(body, cache)?;
    let admitted = (|| {
        let (slots, end) = decoded.ok()?;
        (end == body.len() || body.get(end..) == Some(&[0xe1])).then_some(())?;
        let radius_field = prototype.field("radius")?;
        let (radius, end) = scalar::decode_named_surface_radius(&radius_field.body, 0, cache)?;
        matches!(
            radius_field.body.get(end..),
            Some([] | [0xe1] | [0xf1, 0xf7, 0x0e])
        )
        .then_some(())?;
        bounded_frame(envelope, slots, radius)
    })();
    Some((
        admitted.map_or(Admission::Refused, Admission::Admitted),
        floating,
    ))
}
