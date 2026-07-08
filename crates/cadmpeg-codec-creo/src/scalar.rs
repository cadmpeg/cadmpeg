// SPDX-License-Identifier: Apache-2.0
//! Context-independent PSB scalar forms with fully defined IEEE mappings.

use crate::psb::short_form_float;

/// Decode one scalar whose prefix has a complete byte-to-IEEE definition.
/// Returns the value and the first unread offset; `None` leaves the caller's
/// record grammar responsible for the byte.
pub fn decode(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    match head {
        0x0f | 0xe6 => Some((0.0, offset + 1)),
        0xe4 => Some((1.0, offset + 1)),
        0x29 | 0x2a | 0x2e | 0x2f | 0x42 | 0x43 | 0x47 | 0x48 => short_form_float(data, offset),
        0x46 => ieee8(data, offset, 0x40),
        0x71 => ieee8(data, offset, 0x3f),
        0x2d => ieee8(data, offset, 0xc0),
        0x6a => ieee7(data, offset, 0x40),
        0xa3 => ieee7(data, offset, 0xc0),
        0xb9 | 0xd3 | 0xdf | 0xaf | 0xb0 | 0xb1 | 0xbf => ieee7(data, offset, 0xbf),
        0x41 | 0x4b | 0x66 | 0x67 | 0x68 | 0x77 | 0x82..=0x8f => ieee7(data, offset, 0x3f),
        _ => None,
    }
}

fn ieee8(data: &[u8], offset: usize, first: u8) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 8)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 8))
}
fn ieee7(data: &[u8], offset: usize, first: u8) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1..7].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_defined_ieee_forms() {
        assert_eq!(decode(&[0xe4], 0), Some((1.0, 1)));
        assert_eq!(decode(&[0x46, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((3.0, 8)));
        assert_eq!(decode(&[0x6a, 0x08, 0, 0, 0, 0, 0], 0), Some((3.0, 7)));
        assert_eq!(decode(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((-3.0, 8)));
    }
}
