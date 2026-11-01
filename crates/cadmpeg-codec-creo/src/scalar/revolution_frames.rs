// SPDX-License-Identifier: Apache-2.0
//! Complete rotation frames with sub-unit direction literals.

use super::{ieee8, rotation_plane_axes, ScalarCache, StatefulScalarKind};
use cadmpeg_core::decode::View;
use cadmpeg_ir::units::FiniteVector;

pub(crate) fn prefix(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<Result<(FiniteVector<12>, usize), &'static str>> {
    let mut values = [0.0; 12];
    let mut cursor = 0;
    let mut slot = 0usize;
    let mut tiny_direction = false;
    while slot < 9 {
        let token = View::over_retained(body).child(cursor, body.len())?.u8()?;
        if !matches!(token, 0x0f | 0x10 | 0x18 | 0x19 | 0x32 | 0xe4..=0xe6) {
            return None;
        }
        tiny_direction |= matches!(token, 0x19 | 0x32);
        let (value, count, end) = run(body, cursor, slot.checked_sub(1).map(|i| values[i]), cache)?;
        let next = slot.checked_add(count)?;
        values.get_mut(slot..next)?.fill(value);
        (next <= 9).then_some(())?;
        slot = next;
        cursor = end;
    }
    tiny_direction.then_some(())?;
    let complete = (|| {
        rotation_plane_axes(&values)?;
        while slot < values.len() {
            let (value, count, end) = run(body, cursor, Some(values[slot - 1]), cache)?;
            let next = slot.checked_add(count)?;
            values.get_mut(slot..next)?.fill(value);
            slot = next;
            cursor = end;
        }
        Some((FiniteVector::new(values)?, cursor))
    })();
    Some(
        complete
            .ok_or("sub-unit rotation requires a right-handed unit matrix and twelve finite slots"),
    )
}

pub(crate) fn run(
    body: &[u8],
    cursor: usize,
    previous: Option<f64>,
    cache: &ScalarCache,
) -> Option<(f64, usize, usize)> {
    let token = View::over_retained(body).child(cursor, body.len())?.u8()?;
    let (value, end) = match token {
        0x19 | 0x32 => ieee8(body, cursor, if token == 0x19 { 0x3c } else { 0xbc })?,
        0x1b | 0x34 => {
            let mut raw = [0u8; 8];
            raw[0] = if token == 0x1b { 0x3c } else { 0xbc };
            raw[1..3].copy_from_slice(body.get(cursor + 1..cursor + 3)?);
            (super::be_f64(raw), cursor + 3)
        }
        _ => {
            return super::decode_stateful_scalar_run(
                body,
                cursor,
                previous,
                cache,
                StatefulScalarKind::Plane,
            )
        }
    };
    value.is_finite().then_some((value, 1, end))
}

pub(super) fn supports(values: FiniteVector<12>) -> Option<FiniteVector<12>> {
    let m = values.get();
    FiniteVector::new([
        m[0], m[3], m[6], 0.0, 0.0, 0.0, m[1], m[4], m[7], m[9], m[10], m[11],
    ])
}

pub(crate) fn parameter_literal(body: &[u8], cursor: usize) -> Option<(f64, usize)> {
    let mut view = View::over_retained(body);
    view.seek(cursor)?;
    let constant = |tag| match tag {
        0x0e => Some(0.5_f64),
        0x0f => Some(1.0),
        0x10 => Some(-1.0),
        0x11 => Some(std::f64::consts::PI),
        0x12 => Some(std::f64::consts::FRAC_PI_2),
        0x13 => Some(std::f64::consts::TAU),
        0x14 => Some(3.0 * std::f64::consts::FRAC_PI_2),
        0x15 => Some(-std::f64::consts::PI),
        0x16 => Some(-std::f64::consts::FRAC_PI_2),
        _ => None,
    };
    let value = match view.u8()? {
        0x00 => {
            let mut raw = constant(view.u8()?)?.to_be_bytes();
            raw[7] = view.u8()?;
            super::be_f64(raw)
        }
        tag @ 0x11..=0x16 => constant(tag)?,
        _ => return None,
    };
    Some((value, view.read_len()))
}

pub(crate) fn cylinder_prefix(
    body: &[u8],
    cache: &ScalarCache,
) -> Option<Result<(FiniteVector<12>, usize), &'static str>> {
    if let Some(frame) = prefix(body, cache) {
        return Some(frame);
    }
    let mut values = [0.0; 12];
    let mut slot = 0usize;
    let mut cursor = 0;
    let mut repeated = false;
    while slot < 9 {
        let token = View::over_retained(body).child(cursor, body.len())?.u8()?;
        if !matches!(token, 0x0f | 0x10 | 0x18 | 0xe4..=0xe6) {
            return None;
        }
        repeated |= matches!(token, 0xe4..=0xe6);
        let (value, count, end) = run(body, cursor, slot.checked_sub(1).map(|i| values[i]), cache)?;
        let next = slot.checked_add(count)?;
        values.get_mut(slot..next)?.fill(value);
        slot = next;
        cursor = end;
    }
    repeated.then_some(())?;
    super::orthogonal_plane_columns(&values)?;
    let complete = (|| {
        rotation_plane_axes(&values)?;
        while slot < 12 {
            let (value, count, end) = run(body, cursor, Some(values[slot - 1]), cache)?;
            let next = slot.checked_add(count)?;
            values.get_mut(slot..next)?.fill(value);
            slot = next;
            cursor = end;
        }
        Some((FiniteVector::new(values)?, cursor))
    })();
    Some(
        complete
            .ok_or("cylinder rotation requires a right-handed unit matrix and twelve finite slots"),
    )
}
