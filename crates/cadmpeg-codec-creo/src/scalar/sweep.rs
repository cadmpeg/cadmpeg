// SPDX-License-Identifier: Apache-2.0
//! Scalars in complete angular two-chart and analytic-envelope streams.

use super::{decode_stateful_scalar_run, ScalarCache, StatefulScalarKind};
use cadmpeg_core::decode::View;

pub(crate) fn angle(token: u8) -> Option<f64> {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    match token {
        0x11 => Some(PI),
        0x12 => Some(FRAC_PI_2),
        0x13 => Some(TAU),
        0x14 => Some(3.0 * FRAC_PI_2),
        0x15 => Some(-PI),
        0x16 => Some(-FRAC_PI_2),
        _ => None,
    }
}

pub(crate) fn run(
    body: &[u8],
    cursor: usize,
    previous: Option<f64>,
    cache: &ScalarCache,
) -> Option<(f64, usize, usize)> {
    let mut view = View::over_retained(body).child(cursor, body.len())?;
    let token = view.u8()?;
    if let Some(value) = angle(token) {
        return Some((value, 1, cursor + 1));
    }
    if token == 0x00 {
        let literal = view.u8()?;
        let value = angle(literal).or(match literal {
            0x04 => Some(f64::from_bits(0x3fef_ffff_ffff_ffff)),
            0x0e => Some(0.5),
            0x0f => Some(1.0),
            0x10 => Some(-1.0),
            0x18 => Some(0.0),
            _ => None,
        })?;
        let low = view.u8()?;
        return Some((
            f64::from_bits((value.to_bits() & !0xff) | u64::from(low)),
            1,
            cursor + 3,
        ));
    }
    if matches!(token, 0x19 | 0x32) {
        let mut image = [0; 8];
        image[0] = if token == 0x19 { 0x3c } else { 0xbc };
        image[1..].copy_from_slice(view.take(7)?);
        return Some((View::f64_be_at(&image, 0)?, 1, cursor + 8));
    }
    if token == 0x1d {
        let mut image = [0; 8];
        image[0] = 0x3c;
        image[1..7].copy_from_slice(view.take(6)?);
        return Some((View::f64_be_at(&image, 0)?, 1, cursor + 7));
    }
    if token == 0x1e {
        let mut image = [0; 8];
        image[0] = 0x3d;
        image[1..].copy_from_slice(view.take(7)?);
        return Some((View::f64_be_at(&image, 0)?, 1, cursor + 8));
    }
    if matches!(token, 0x1a | 0x1b) {
        let mut image = [0; 8];
        image[0] = 0x3c;
        image[1] = view.u8()?;
        image[2] = view.u8()?;
        if token == 0x1a {
            let fill = image[2];
            image[3..].fill(fill);
        }
        return Some((View::f64_be_at(&image, 0)?, 1, cursor + 3));
    }
    decode_stateful_scalar_run(body, cursor, previous, cache, StatefulScalarKind::Pcurve)
}
