// SPDX-License-Identifier: Apache-2.0
//! PSB scalar forms with context-independent IEEE-754 mappings.

use std::collections::{BTreeMap, HashSet};

use crate::psb::{compact_int, short_form_float};

/// Section-local dictionary formed by distinct raw `0x46` token images.
#[derive(Debug, Clone, Default)]
pub struct ScalarCache {
    entries: Vec<CacheEntry>,
    paired_byte_1_by_tail: BTreeMap<[u8; 6], u8>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: f64,
}

impl ScalarCache {
    /// Build the dictionary in first-appearance order from every complete
    /// eight-byte sequence beginning with `0x46` in one section.
    pub fn from_section(section: &[u8]) -> Self {
        let mut entries = Vec::<CacheEntry>::new();
        let mut seen = HashSet::<[u8; 8]>::new();
        let mut paired_byte_1_by_tail = BTreeMap::new();
        for offset in 0..section.len() {
            if section[offset] != 0x46 {
                continue;
            }
            let Some(bytes) = section.get(offset..offset + 8) else {
                continue;
            };
            let raw: [u8; 8] = bytes.try_into().expect("bounded eight-byte slice");
            if !seen.insert(raw) {
                continue;
            }
            let mut ieee = raw;
            ieee[0] = 0x40;
            paired_byte_1_by_tail
                .entry(raw[2..].try_into().expect("six-byte cache tail"))
                .or_insert(raw[1]);
            entries.push(CacheEntry {
                value: f64::from_be_bytes(ieee),
            });
        }
        Self {
            entries,
            paired_byte_1_by_tail,
        }
    }

    fn value(&self, index: u32) -> Option<f64> {
        self.entries
            .get(usize::try_from(index).ok()?)
            .map(|entry| entry.value)
    }

    fn paired_byte_1(&self, tail: &[u8]) -> Option<u8> {
        self.paired_byte_1_by_tail
            .get(<&[u8; 6]>::try_from(tail).ok()?)
            .copied()
    }
}

const LANE_OPENERS: &[u8] = &[
    0x0d, 0x0e, 0x0f, 0x18, 0x29, 0x2d, 0x2e, 0x2f, 0x41, 0x42, 0x46, 0x47, 0x48, 0x4b, 0x5e, 0x66,
    0x68, 0x6a, 0x71, 0x74, 0x76, 0x77, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a,
    0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x9e, 0xa1, 0xa2, 0xa3, 0xaf, 0xb0, 0xb1, 0xb7, 0xb9,
    0xbf, 0xd3, 0xd7, 0xde, 0xdf, 0xe4, 0xd1, 0xe6, 0xe8, 0xb3,
];

/// Decode one scalar in a row or `f9` scalar lane using its section cache.
pub fn decode_in_lane(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    match *data.get(offset)? {
        0x18 => {
            let next = *data.get(offset + 1)?;
            if LANE_OPENERS.contains(&next)
                || matches!(next, 0xe0..=0xe3 | 0xf1 | 0xf2 | 0xf7 | 0xf8)
            {
                return Some((0.0, offset + 1));
            }
            let (index, end) = compact_int(data, offset + 1);
            (end > offset + 1).then(|| cache.value(index).map(|value| (value, end)))?
        }
        0x9e | 0xa3 => {
            let tail = data.get(offset + 1..offset + 7)?;
            let byte_1 = cache.paired_byte_1(tail)?;
            let mut raw = [0; 8];
            raw[0] = if data[offset] == 0x9e { 0x40 } else { 0xc0 };
            raw[1] = byte_1;
            raw[2..].copy_from_slice(tail);
            Some((f64::from_be_bytes(raw), offset + 7))
        }
        0x76 | 0xb3 => {
            let tail = data.get(offset + 1..offset + 7)?;
            let mut raw = [0; 8];
            raw[..2].copy_from_slice(if data[offset] == 0x76 {
                &[0x3f, 0xeb]
            } else {
                &[0xbf, 0xe0]
            });
            raw[2..].copy_from_slice(tail);
            Some((f64::from_be_bytes(raw), offset + 7))
        }
        0xe8 if data.get(offset + 1) == Some(&0) => Some((1.0, offset + 2)),
        _ => decode(data, offset),
    }
}

/// Decode one scalar in a positional surface or curve row lane.
///
/// Positional rows store `0x71` as a seven-byte sub-one IEEE form with an
/// implicit zero low byte. Named scalar fields use the eight-byte `0x71`
/// form handled by [`decode_in_lane`].
pub fn decode_in_row_lane(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x0e) {
        return Some((-0.5, offset + 1));
    }
    if data.get(offset) == Some(&0x71) {
        return ieee7(data, offset, 0x3f);
    }
    decode_in_lane(data, offset, cache)
}

/// Decode one scalar in a positional surface-row lane.
pub fn decode_in_surface_row_lane(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x18) && matches!(data.get(offset + 1), Some(0x73 | 0xa0 | 0xbb)) {
        return Some((0.0, offset + 1));
    }
    if data.get(offset) == Some(&0xa0) {
        let tail = data.get(offset + 1..offset + 7)?;
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0xc0, 0x15]);
        raw[2..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    decode_in_row_lane(data, offset, cache)
}

/// Decode the first coordinate of a tabulated-cylinder directrix control point.
///
/// This lane adds `0x4a`, whose six-byte payload completes a negative IEEE
/// value with an implicit `0xc0` high byte and zero low byte.
pub fn decode_tabulated_cylinder_first_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x4a) {
        return ieee7(data, offset, 0xc0);
    }
    decode_in_surface_row_lane(data, offset, cache)
}

/// Decode the second coordinate of a tabulated-cylinder directrix control point.
///
/// Positive DICT tokens encode the first two IEEE bytes as `0x3f75 + prefix`;
/// their six-byte payload supplies the remaining bytes.
pub fn decode_tabulated_cylinder_second_coordinate(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    if matches!(head, 0x78..=0x8a | 0xa1..=0xa3) {
        let high = 0x3f75_u16 + u16::from(head);
        let tail = data.get(offset + 1..offset + 7)?;
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&high.to_be_bytes());
        raw[2..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    decode_in_surface_row_lane(data, offset, cache)
}

/// Decode one scalar in the positive seven-byte DICT lane.
///
/// The enclosing record grammar must establish this lane. Several prefix
/// bytes have different meanings in positional row and generic scalar lanes.
pub fn decode_positive_dict(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    let (byte_0, byte_1) = match *data.get(offset)? {
        0x71 => (0x3f, 0xe6),
        0x74 => (0x3f, 0xe9),
        0x76 => (0x3f, 0xeb),
        0x81 => (0x3f, 0xf6),
        0x8b => (0x40, 0x00),
        0x90 => (0x40, 0x05),
        0x91 => (0x40, 0x06),
        0xa1 => (0x40, 0x16),
        0xa2 => (0x40, 0x17),
        0xb7 => (0x3f, 0xe4),
        _ => return None,
    };
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = byte_0;
    raw[1] = byte_1;
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

/// Decode one scalar with a defined byte-to-IEEE mapping.
///
/// Returns the value and first unread offset. Returns `None` when the prefix
/// requires interpretation by the enclosing record grammar or input is
/// truncated.
pub fn decode(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    let head = *data.get(offset)?;
    match head {
        0x0d => Some((-1.0, offset + 1)),
        0x0f | 0xe6 => Some((0.0, offset + 1)),
        0xe4 => Some((1.0, offset + 1)),
        0x29 | 0x2a | 0x2e | 0x2f | 0x42 | 0x43 | 0x47 | 0x48 => short_form_float(data, offset),
        0x46 => ieee8(data, offset, 0x40),
        0x71 => ieee8(data, offset, 0x3f),
        0x2d => ieee8(data, offset, 0xc0),
        0x6a => ieee7(data, offset, 0x40),
        0x5e => ieee7_with_prefix(data, offset, 0x3f, 0xd3),
        0xa3 => ieee7(data, offset, 0xc0),
        0xb9 | 0xd1 | 0xd3 | 0xde | 0xdf | 0xaf | 0xb0 | 0xb1 | 0xbf => ieee7(data, offset, 0xbf),
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

fn ieee7_with_prefix(data: &[u8], offset: usize, first: u8, second: u8) -> Option<(f64, usize)> {
    let tail = data.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1] = second;
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_defined_ieee_forms() {
        assert_eq!(decode(&[0xe4], 0), Some((1.0, 1)));
        assert_eq!(decode(&[0x0d], 0), Some((-1.0, 1)));
        assert_eq!(decode(&[0x46, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((3.0, 8)));
        assert_eq!(decode(&[0x6a, 0x08, 0, 0, 0, 0, 0], 0), Some((3.0, 7)));
        assert_eq!(
            decode(&[0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x2c], 0),
            Some((
                f64::from_be_bytes([0x3f, 0xd3, 0x33, 0x33, 0x33, 0x33, 0x33, 0x2c]),
                7
            ))
        );
        assert_eq!(decode(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((-3.0, 8)));
        assert_eq!(
            decode(&[0xde, 0x5c, 0xfa, 0x99, 0x80, 0x36, 0x84], 0),
            Some((
                f64::from_be_bytes([0xbf, 0x5c, 0xfa, 0x99, 0x80, 0x36, 0x84, 0]),
                7
            ))
        );
    }

    #[test]
    fn decodes_tabulated_cylinder_coordinate_lanes() {
        let cache = ScalarCache::default();
        let first = [0x4a, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00];
        let second = [0x7f, 0x24, 0x57, 0x89, 0x13, 0x66, 0x08];
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&first, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00, 0]),
                7
            ))
        );
        assert_eq!(
            decode_tabulated_cylinder_second_coordinate(&second, 0, &cache),
            Some((
                f64::from_be_bytes([0x3f, 0xf4, 0x24, 0x57, 0x89, 0x13, 0x66, 0x08]),
                7
            ))
        );
    }

    #[test]
    fn section_cache_uses_unique_raw_tokens_in_first_appearance_order() {
        let first = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
        let second = [0x46, 0x10, 0, 0, 0, 0, 0, 0];
        let mut section = vec![0xaa];
        section.extend_from_slice(&first);
        section.extend_from_slice(&first);
        section.extend_from_slice(&second);
        let cache = ScalarCache::from_section(&section);

        assert_eq!(decode_in_lane(&[0x18, 0], 0, &cache), Some((3.0, 2)));
        assert_eq!(decode_in_lane(&[0x18, 1], 0, &cache), Some((4.0, 2)));
    }

    #[test]
    fn lane_zero_does_not_consume_the_following_scalar_opener() {
        let cache = ScalarCache::default();
        assert_eq!(decode_in_lane(&[0x18, 0xe4], 0, &cache), Some((0.0, 1)));
        assert_eq!(decode_in_lane(&[0x18, 0xe4], 1, &cache), Some((1.0, 2)));
        assert_eq!(decode_in_lane(&[0x18, 0x0d], 0, &cache), Some((0.0, 1)));
        assert_eq!(decode_in_row_lane(&[0x18, 0x0e], 0, &cache), Some((0.0, 1)));
        assert_eq!(decode_in_lane(&[0x18, 0x18, 0], 0, &cache), Some((0.0, 1)));
    }

    #[test]
    fn lane_zero_does_not_consume_the_following_named_record() {
        let cache = ScalarCache::default();
        assert_eq!(decode_in_lane(&[0x18, 0xe0], 0, &cache), Some((0.0, 1)));
    }

    #[test]
    fn paired_negative_lane_uses_matching_positive_cache_tail() {
        let cache = ScalarCache::from_section(&[0x46, 0x08, 1, 2, 3, 4, 5, 6]);
        let expected = f64::from_be_bytes([0xc0, 0x08, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            decode_in_lane(&[0xa3, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((expected, 7))
        );
    }

    #[test]
    fn decodes_saved_spline_tangent_dict_forms() {
        let cache = ScalarCache::default();
        let negative = [0xb3, 0, 0, 0, 0, 0, 0];
        let positive = [0x76, 0xb6, 0x7a, 0xe8, 0x58, 0x4c, 0x9a];

        assert_eq!(decode_in_lane(&negative, 0, &cache), Some((-0.5, 7)));
        let (value, end) = decode_in_lane(&positive, 0, &cache).expect("positive tangent");
        assert_eq!(end, 7);
        assert!((value - 3.0_f64.sqrt() / 2.0).abs() < 3e-15);
    }

    #[test]
    fn paired_positive_lane_uses_matching_cache_exponent() {
        let cache = ScalarCache::from_section(&[0x46, 0x13, 1, 2, 3, 4, 5, 6]);
        let expected = f64::from_be_bytes([0x40, 0x13, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((expected, 7))
        );
        assert_eq!(
            decode_in_lane(&[0x18, 0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((0.0, 1))
        );
    }

    #[test]
    fn paired_cache_tail_keeps_its_first_exponent() {
        let cache = ScalarCache::from_section(&[
            0x46, 0x08, 1, 2, 3, 4, 5, 6, 0x46, 0x13, 1, 2, 3, 4, 5, 6,
        ]);
        let expected = f64::from_be_bytes([0x40, 0x08, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
            Some((expected, 7))
        );
    }

    #[test]
    fn row_lane_uses_seven_byte_0x71_without_consuming_the_next_scalar() {
        let cache = ScalarCache::default();
        let data = [0x71, 0xf0, 0, 0, 0, 0, 0, 0xe4];
        assert_eq!(decode_in_row_lane(&data, 0, &cache), Some((1.0, 7)));
        assert_eq!(decode_in_row_lane(&data, 7, &cache), Some((1.0, 8)));
        assert_eq!(
            decode_in_lane(&data, 0, &cache).map(|(_, end)| end),
            Some(8)
        );
    }

    #[test]
    fn surface_row_lane_decodes_negative_a0_dict_form() {
        let cache = ScalarCache::default();
        let data = [0xa0, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0x5c, 0xe4];
        assert_eq!(
            decode_in_surface_row_lane(&data, 0, &cache),
            Some((
                f64::from_be_bytes([0xc0, 0x15, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0x5c]),
                7
            ))
        );
        assert_eq!(decode_in_surface_row_lane(&data, 7, &cache), Some((1.0, 8)));
    }

    #[test]
    fn surface_row_zero_does_not_consume_a_surface_only_opener() {
        let cache = ScalarCache::default();

        for opener in [0x73, 0xa0, 0xbb] {
            assert_eq!(
                decode_in_surface_row_lane(&[0x18, opener, 0, 0, 0, 0, 0, 0], 0, &cache),
                Some((0.0, 1))
            );
        }
    }

    #[test]
    fn positive_dict_lane_decodes_cone_half_angles() {
        let forty_five_degrees = [0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23];
        let eighty_degrees = [0x81, 0x57, 0x18, 0x4a, 0xe7, 0x44, 0x8d];
        let other_angle = [0xb7, 0x5e, 0x8a, 0x1c, 0xf2, 0x17, 0x1e];

        assert_eq!(
            decode_positive_dict(&forty_five_degrees, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23]),
                7
            ))
        );
        assert_eq!(
            decode_positive_dict(&eighty_degrees, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xf6, 0x57, 0x18, 0x4a, 0xe7, 0x44, 0x8d]),
                7
            ))
        );
        assert_eq!(
            decode_positive_dict(&other_angle, 0),
            Some((
                f64::from_be_bytes([0x3f, 0xe4, 0x5e, 0x8a, 0x1c, 0xf2, 0x17, 0x1e]),
                7
            ))
        );
    }

    #[test]
    fn row_lane_decodes_negative_half_literal() {
        let cache = ScalarCache::default();

        assert_eq!(
            decode_in_row_lane(&[0x0e, 0x18], 0, &cache),
            Some((-0.5, 1))
        );
    }
}
