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
    if token == 0xea {
        let mut image = [0; 8];
        image[..3].copy_from_slice(view.take(3)?);
        return Some((View::f64_be_at(&image, 0)?, 1, cursor + 4));
    }
    if matches!(token, 0x1a | 0x1b | 0x33) {
        let mut image = [0; 8];
        image[0] = if token == 0x33 { 0xbc } else { 0x3c };
        image[1] = view.u8()?;
        image[2] = view.u8()?;
        if matches!(token, 0x1a | 0x33) {
            let fill = image[2];
            image[3..].fill(fill);
        }
        return Some((View::f64_be_at(&image, 0)?, 1, cursor + 3));
    }
    decode_stateful_scalar_run(body, cursor, previous, cache, StatefulScalarKind::Pcurve)
}

#[cfg(test)]
mod tests {
    use super::{run, ScalarCache};

    #[test]
    fn angular_stream_reads_negative_repeat_fill_and_three_byte_images() {
        let cache = ScalarCache::default();
        for (body, bits) in [
            (&[0x33, 0xd3, 0x99][..], 0xbcd3_9999_9999_9999_u64),
            (&[0x1a, 0xd3, 0x99][..], 0x3cd3_9999_9999_9999),
            (&[0xea, 0x39, 0xa6, 0x00][..], 0x39a6_0000_0000_0000),
            (&[0xea, 0xb9, 0x70, 0x01][..], 0xb970_0100_0000_0000),
        ] {
            let (value, repeat, end) = run(body, 0, None, &cache).expect("complete scalar");
            assert_eq!((value.to_bits(), repeat, end), (bits, 1, body.len()));
        }
        assert!(run(&[0x33, 0xd3], 0, None, &cache).is_none());
        assert!(run(&[0xea, 0x39, 0xa6], 0, None, &cache).is_none());
    }
}
