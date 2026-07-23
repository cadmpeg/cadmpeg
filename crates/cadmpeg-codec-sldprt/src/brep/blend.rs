// SPDX-License-Identifier: Apache-2.0
//! Constant-radius rolling-ball surface carriers.
//!
//! A `00 38` record names two support surfaces and a spine curve. Equal offset
//! magnitudes define a circular rolling-ball envelope; their signs and two unit
//! scalars select the support-normal sides.

use std::collections::HashMap;

use cadmpeg_ir::be::{f64_at, u16_at};

use super::LEN_TO_MM;

/// One exact constant-radius blend construction.
#[derive(Debug, Clone)]
pub(crate) struct BlendCarrier {
    /// Stream-local surface carrier attribute.
    pub attr: u16,
    /// Tag-byte offset in the stream.
    pub offset: usize,
    /// Ordered support-surface carrier attributes.
    pub supports: [u16; 2],
    /// Stored center/spine curve attribute.
    pub spine: u16,
    /// Signed rolling-ball radius in millimetres.
    pub signed_radius: f64,
    /// Whether each support uses the opposite natural-normal side.
    pub reversed: [bool; 2],
}

fn parse_blend(bytes: &[u8], offset: usize) -> Option<BlendCarrier> {
    if bytes.get(offset..offset + 2) != Some(&[0x00, 0x38]) {
        return None;
    }
    let mut body = offset + 2;
    if bytes.get(body) == Some(&0xff) {
        body += 1;
    }
    let attr = u16_at(bytes, body)?;
    if attr == 0 || !matches!(bytes.get(body + 16), Some(0x2b | 0x2d)) {
        return None;
    }
    let payload = body + 17;
    if bytes.get(payload) != Some(&0x52) {
        return None;
    }
    let supports = [u16_at(bytes, payload + 1)?, u16_at(bytes, payload + 3)?];
    let spine = u16_at(bytes, payload + 5)?;
    if supports.contains(&0) || spine == 0 {
        return None;
    }
    let values = payload + 7;
    let first_radius = f64_at(bytes, values)?;
    let second_radius = f64_at(bytes, values + 8)?;
    let first_side = f64_at(bytes, values + 16)?;
    let second_side = f64_at(bytes, values + 24)?;
    if !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius.abs() <= f64::EPSILON
        || (first_radius.abs() - second_radius.abs()).abs() > 1.0e-12
        || (first_side.abs() - 1.0).abs() > 1.0e-12
        || (second_side.abs() - 1.0).abs() > 1.0e-12
    {
        return None;
    }
    Some(BlendCarrier {
        attr,
        offset,
        supports,
        spine,
        signed_radius: first_radius * LEN_TO_MM,
        reversed: [
            first_side.is_sign_negative(),
            second_side.is_sign_negative()
                ^ (first_radius.is_sign_negative() != second_radius.is_sign_negative()),
        ],
    })
}

/// Scan exact constant-radius rolling-ball carriers by surface attribute.
pub(crate) fn scan_blend_carriers(bytes: &[u8]) -> HashMap<u16, BlendCarrier> {
    let mut out = HashMap::new();
    for offset in 0..bytes.len().saturating_sub(57) {
        if let Some(carrier) = parse_blend(bytes, offset) {
            out.entry(carrier.attr).or_insert(carrier);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_constant_radius_support_and_spine_payload() {
        let mut bytes = vec![0x00, 0x38, 0xff];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        for reference in [1u16, 2, 3, 4, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [-0.0005f64, -0.0005, 1.0, -1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }

        let carrier = scan_blend_carriers(&bytes)
            .remove(&9)
            .expect("blend carrier");
        assert_eq!(carrier.offset, 0);
        assert_eq!(carrier.supports, [11, 12]);
        assert_eq!(carrier.spine, 13);
        assert_eq!(carrier.signed_radius, -0.5);
        assert_eq!(carrier.reversed, [false, true]);
    }

    #[test]
    fn unequal_radius_magnitudes_are_not_a_constant_radius_blend() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [-0.0005f64, -0.0006, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert!(scan_blend_carriers(&bytes).is_empty());
    }

    #[test]
    fn opposite_offset_sign_reverses_the_second_support() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [0.0005f64, -0.0005, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            scan_blend_carriers(&bytes).get(&9).expect("blend").reversed,
            [false, true]
        );
    }
}
