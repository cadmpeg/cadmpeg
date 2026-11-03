// SPDX-License-Identifier: Apache-2.0
//! Signed-basis bend compounds with positive angular lower bounds.

use super::{PositionalCylinderFrame, EPS_SURFACE_AGREEMENT};
use crate::{psb, scalar};
use cadmpeg_core::decode::View;

struct Envelope {
    angular: [f64; 2],
    axial: [f64; 2],
    corners: [[f64; 3]; 2],
    end: usize,
}

fn envelope(body: &[u8], cache: &scalar::ScalarCache) -> Option<Envelope> {
    fn values<const N: usize>(
        body: &[u8],
        cursor: &mut usize,
        previous: &mut Option<f64>,
        cache: &scalar::ScalarCache,
    ) -> Option<[f64; N]> {
        let mut result = [0.0; N];
        let mut slot = 0usize;
        while slot < N {
            let (value, count, end) = scalar::sweep::run(body, *cursor, *previous, cache)?;
            let next = slot.checked_add(count)?;
            result.get_mut(slot..next)?.fill(value);
            slot = next;
            *cursor = end;
            *previous = Some(value);
        }
        Some(result)
    }
    // The angular lower bound is an explicit literal. A leading control token
    // reads as zero in the standard cylinder envelope, so it does not open a
    // bend envelope.
    (!matches!(body.first(), Some(0x0f | 0x10 | 0x12 | 0x18 | 0xe4..=0xe7))).then_some(())?;
    let mut cursor = 0;
    let mut previous = None;
    let [lower, axial_lower] = values::<2>(body, &mut cursor, &mut previous, cache)?;
    let mut view = View::over_retained(body);
    view.seek(cursor)?;
    (lower > 0.0 && lower < std::f64::consts::FRAC_PI_2 && view.u8() == Some(0x12)).then_some(())?;
    cursor = view.read_len();
    let upper = std::f64::consts::FRAC_PI_2;
    previous = Some(upper);
    let [axial_upper, x0, y0, z0, x1, y1, z1] =
        values::<7>(body, &mut cursor, &mut previous, cache)?;
    (axial_lower != axial_upper).then_some(())?;
    view.seek(cursor)?;
    if view.u8() == Some(psb::token::ENTITY_REF) {
        cursor = psb::reference_id(body, cursor + 1).ok()?.1;
    }
    Some(Envelope {
        angular: [lower, upper],
        axial: [axial_lower, axial_upper],
        corners: [[x0, y0, z0], [x1, y1, z1]],
        end: cursor,
    })
}

fn agrees(envelope: &Envelope, frame: PositionalCylinderFrame) -> Option<()> {
    let origin = frame.frame().origin();
    let axis = frame.frame().axis();
    let u = frame.frame().ref_direction();
    let v = crate::vecmath::cross(axis, u);
    u.into_iter()
        .chain(v)
        .chain(axis)
        .all(|value| matches!(value, -1.0 | 0.0 | 1.0))
        .then_some(())?;
    let radius = frame.radius().get();
    let scale = origin
        .into_iter()
        .chain(envelope.corners.into_iter().flatten())
        .chain([radius])
        .map(f64::abs)
        .fold(1.0, f64::max);
    for coordinate in 0..3 {
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        for angle in envelope.angular {
            for height in envelope.axial {
                let value = origin[coordinate]
                    + radius * (u[coordinate] * angle.cos() + v[coordinate] * angle.sin())
                    + axis[coordinate] * height;
                minimum = minimum.min(value);
                maximum = maximum.max(value);
            }
        }
        let stored = [
            envelope.corners[0][coordinate].min(envelope.corners[1][coordinate]),
            envelope.corners[0][coordinate].max(envelope.corners[1][coordinate]),
        ];
        ([minimum, maximum]
            .into_iter()
            .zip(stored)
            .all(|(a, b)| a.is_finite() && (a - b).abs() <= EPS_SURFACE_AGREEMENT * scale))
        .then_some(())?;
    }
    Some(())
}

pub(super) fn inline_body(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<(Option<PositionalCylinderFrame>, usize)> {
    let envelope = envelope(body, cache)?;
    let mut view = View::over_retained(body);
    view.seek(envelope.end)?;
    (view.u8()? == psb::token::COMPOUND_CLOSE).then_some(())?;
    let start = view.read_len();
    let mut candidates = (start..body.len()).filter_map(|radius_start| {
        let slots = scalar::decode_compact_plane_rotation(body.get(start..radius_start)?, cache)?
            .ok()?
            .get();
        let (radius, close) =
            scalar::decode_inline_surface_suffix_scalar(body, radius_start, cache)?;
        view.seek(close)?;
        (view.u8() == Some(psb::token::COMPOUND_CLOSE)).then_some(())?;
        let [u, _, v, origin] = crate::vecmath::local_system_lanes(slots);
        let axis = crate::vecmath::cross(u, v);
        let frame = PositionalCylinderFrame::new(
            origin,
            axis,
            u,
            radius,
            Some((envelope.axial[1] - envelope.axial[0]).abs()),
        );
        Some((
            frame.filter(|frame| agrees(&envelope, *frame).is_some()),
            close,
        ))
    });
    let result = candidates.next()?;
    candidates.next().is_none().then_some(result)
}

pub(super) fn named_frame(
    parameter: &super::SurfaceParameterRecord,
    slots: cadmpeg_ir::units::FiniteVector<12>,
    radius: Option<f64>,
    cache: &scalar::ScalarCache,
) -> Option<super::cylinder_frame_readers::CylinderRotationFrame> {
    let envelope = envelope(&parameter.body, cache)?;
    (envelope.end == parameter.body.len()).then_some(())?;
    let (u, axis) = scalar::rotation_plane_axes(slots.as_raw())?;
    let m = slots.get();
    Some(
        super::cylinder_frame_readers::CylinderRotationFrame::from_frame((|| {
            let radius = radius?;
            let frame = PositionalCylinderFrame::new(
                [m[9], m[10], m[11]],
                axis,
                u,
                radius,
                Some((envelope.axial[1] - envelope.axial[0]).abs()),
            )?;
            agrees(&envelope, frame)?;
            Some(frame)
        })()),
    )
}

pub(super) fn parameter_bounds(
    body: &[u8],
    frame: PositionalCylinderFrame,
) -> Option<[[f64; 2]; 2]> {
    let envelope = envelope(body, &scalar::ScalarCache::default())?;
    let mut view = View::over_retained(body);
    view.seek(envelope.end)?;
    (view.is_empty() || view.u8() == Some(psb::token::COMPOUND_CLOSE)).then_some(())?;
    agrees(&envelope, frame)?;
    Some([envelope.angular, envelope.axial])
}

pub(super) fn named_boundary(body: &[u8], cache: &scalar::ScalarCache) -> Option<usize> {
    let envelope = envelope(body, cache)?;
    let marker = b"\xe0\0srf_prim_ptr(cylinder)\0";
    let mut view = View::over_retained(body);
    view.seek(envelope.end)?;
    (view.take(marker.len())? == marker).then_some(envelope.end)
}
