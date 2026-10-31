// SPDX-License-Identifier: Apache-2.0
//! Complete angular-stream rotations for planar wall charts.

use super::{rotation_plane_axes, sweep, ScalarCache, EPS_SUPPORT_FRAME_AGREEMENT};
use cadmpeg_core::decode::View;
use cadmpeg_ir::units::FiniteVector;

fn directions(body: &[u8], cache: &ScalarCache) -> Option<([f64; 12], usize)> {
    let mut values = [0.0; 12];
    let mut cursor = 0;
    let mut slot = 0usize;
    let mut explicit_direction = false;
    let mut view = View::over_retained(body);
    while slot < 9 {
        view.seek(cursor)?;
        let tag = view.u8()?;
        let (value, count, end) = sweep::run(
            body,
            cursor,
            slot.checked_sub(1).map(|index| values[index]),
            cache,
        )?;
        let next = slot.checked_add(count)?;
        (next <= 9).then_some(())?;
        values.get_mut(slot..next)?.fill(value);
        explicit_direction |= matches!(tag, 0x00 | 0x19..=0xdf);
        cursor = end;
        slot = next;
    }
    explicit_direction.then_some(())?;
    let axis_column = (0..3).any(|column| {
        [0usize, 2].into_iter().any(|axis| {
            (0..3).all(|row| {
                let value = values[row * 3 + column].abs();
                (value - if row == axis { 1.0 } else { 0.0 }).abs() <= EPS_SUPPORT_FRAME_AGREEMENT
            })
        })
    });
    axis_column.then_some(())?;
    rotation_plane_axes(&values)?;
    Some((values, cursor))
}

fn origins(
    body: &[u8],
    cache: &ScalarCache,
    mut values: [f64; 12],
    mut cursor: usize,
) -> Option<(FiniteVector<12>, usize)> {
    let mut slot = 9usize;
    while slot < values.len() {
        let (value, count, end) = sweep::run(body, cursor, Some(values[slot - 1]), cache)?;
        let next = slot.checked_add(count)?;
        values.get_mut(slot..next)?.fill(value);
        cursor = end;
        slot = next;
    }
    Some((FiniteVector::new(values)?, cursor))
}

pub(crate) fn plane_prefix(body: &[u8], cache: &ScalarCache) -> Option<(FiniteVector<12>, usize)> {
    let (values, cursor) = directions(body, cache)?;
    origins(body, cache, values, cursor)
}

pub(crate) fn plane(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<Result<FiniteVector<12>, &'static str>> {
    let (values, cursor) = directions(body, cache)?;
    let complete = origins(body, cache, values, cursor)
        .filter(|(_, end)| *end == body.len())
        .map(|(slots, _)| slots);
    Some(complete.ok_or("angular plane rotation requires exactly twelve finite slots and complete field consumption"))
}

/// The ten angular-stream scalars that open a plane envelope: the parameter
/// rectangle `u0, v0, u1, v1`, then the XYZ minima and the XYZ maxima. Bytes
/// after the tenth scalar (a linkage field) are not read.
pub(crate) fn envelope(body: &[u8], cache: &ScalarCache) -> Option<[f64; 10]> {
    let mut values = [0.0; 10];
    let mut cursor = 0;
    let mut slot = 0usize;
    while slot < values.len() {
        let (value, count, end) = sweep::run(
            body,
            cursor,
            slot.checked_sub(1).map(|index| values[index]),
            cache,
        )?;
        let next = slot.checked_add(count)?;
        values.get_mut(slot..next)?.fill(value);
        cursor = end;
        slot = next;
    }
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

/// Whether the parameter rectangle of a plane envelope, mapped through the
/// chart `origin + u * first + v * second`, has exactly the XYZ minima and
/// maxima the envelope stores.
pub(crate) fn chart_reproduces_envelope(
    envelope: &[f64; 10],
    origin: [f64; 3],
    first: [f64; 3],
    second: [f64; 3],
) -> bool {
    let [u0, v0, u1, v1, ..] = *envelope;
    let corner = |u: f64, v: f64| -> [f64; 3] {
        std::array::from_fn(|axis| origin[axis] + u * first[axis] + v * second[axis])
    };
    let corners = [
        corner(u0, v0),
        corner(u0, v1),
        corner(u1, v0),
        corner(u1, v1),
    ];
    (0..3).all(|axis| {
        let low = corners
            .iter()
            .map(|point| point[axis])
            .fold(f64::INFINITY, f64::min);
        let high = corners
            .iter()
            .map(|point| point[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        [(low, envelope[4 + axis]), (high, envelope[7 + axis])]
            .into_iter()
            .all(|(chart, stored)| {
                (chart - stored).abs() <= EPS_SUPPORT_FRAME_AGREEMENT * stored.abs().max(1.0)
            })
    })
}

/// Whether a complete angular-stream rotation (nine row-major direction slots,
/// three origin slots) has a chart whose columns reproduce the envelope.
pub(crate) fn plane_reproduces_envelope(slots: &FiniteVector<12>, envelope: &[f64; 10]) -> bool {
    let m = slots.as_raw();
    chart_reproduces_envelope(
        envelope,
        [m[9], m[10], m[11]],
        [m[0], m[3], m[6]],
        [m[1], m[4], m[7]],
    )
}

pub(crate) fn pcurve(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<Result<super::PcurveScalarFrame, &'static str>> {
    let mut frame = super::PcurveScalarFrame {
        values: [0.0; 8],
        tokens: [None; 8],
        token_count: 0,
    };
    let mut slot = 0usize;
    let mut cursor = 0;
    let mut extended_literal = false;
    let mut full_width = false;
    while slot < frame.values.len() {
        let start = cursor;
        let mut view = View::over_retained(body);
        view.seek(start)?;
        if view.take(super::HELD_PCURVE_OPEN.len()) == Some(super::HELD_PCURVE_OPEN) {
            return None;
        }
        view.seek(start)?;
        let tag = view.u8()?;
        full_width |= tag == 0x41;
        extended_literal |= match tag {
            0x1a => true,
            0x00 => view.u8() == Some(0x04),
            _ => false,
        };
        let (value, count, end) = sweep::run(
            body,
            cursor,
            slot.checked_sub(1).map(|index| frame.values[index]),
            cache,
        )?;
        let next = slot.checked_add(count)?;
        frame.values.get_mut(slot..next)?.fill(value);
        *frame.tokens.get_mut(frame.token_count)? = Some(super::StatefulScalarToken {
            value,
            offset: start,
            end,
            repeat: count,
        });
        frame.token_count += 1;
        slot = next;
        cursor = end;
    }
    (extended_literal && cursor == body.len()).then_some(())?;
    Some(if full_width {
        Err("full-width subunit literal has unresolved signed ownership in the angular endpoint body")
    } else {
        Ok(frame)
    })
}
