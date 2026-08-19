// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use crate::scalar::{decode, decode_in_lane, ScalarCache};

/// Exercise Creo datum plane decoders.
pub fn datum(data: &[u8]) {
    let _ = crate::datum::planes(data);
    let _ = crate::datum::named_plane(data);
}

/// Exercise Creo curve prototype extraction.
pub fn curve_prototypes(data: &[u8]) {
    let _ = crate::curve::prototypes(data);
    let _ = crate::curve::expression_records(data);
}

/// Exercise Creo surface namespace row extraction.
pub fn surface_rows(data: &[u8]) {
    let _ = crate::surface::rows(data);
}

/// Exercise Creo positional surface contour-chain extraction.
pub fn surface_contours(data: &[u8]) {
    let _ = crate::surface::contour_records(data);
    let _ = crate::surface::cross_section_contour_records(data);
}

/// Exercise Creo PSB scalar decoding.
pub fn scalar(data: &[u8]) {
    let cache = ScalarCache::from_section(data);
    let mut offset = 0usize;
    while offset < data.len() {
        match decode_in_lane(data, offset, &cache) {
            Some((_, next)) if next > offset => offset = next,
            _ => break,
        }
    }
    let _ = decode(data, 0);
}

/// Exercise Creo compact integer decoding.
pub fn compact_int(data: &[u8]) {
    let _ = crate::psb::compact_int(data, 0);
}

/// Exercise Creo PSB token stream parsing.
pub fn psb_tokens(data: &[u8]) {
    let _ = crate::psb::tokens(data);
}

/// Exercise Creo short-form float decoding.
pub fn short_form_float(data: &[u8]) {
    let _ = crate::psb::is_short_form_float(data.first().copied().unwrap_or(0));
    let _ = crate::psb::short_form_float(data, 0);
}

/// Exercise Creo container scanning.
pub fn container_scan(data: &[u8]) {
    let _ = crate::container::scan_bytes(data.to_vec());
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::datum(&[]);
        super::curve_prototypes(&[]);
        super::surface_rows(&[]);
        super::surface_contours(&[]);
        super::scalar(&[]);
        super::compact_int(&[]);
        super::psb_tokens(&[]);
        super::short_form_float(&[]);
        super::container_scan(&[]);
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::build_prt("1.0", &[]);
        super::datum(&data);
        super::curve_prototypes(&data);
        super::surface_rows(&data);
        super::surface_contours(&data);
        super::scalar(&data);
        super::compact_int(&data);
        super::psb_tokens(&data);
        super::short_form_float(&data);
        super::container_scan(&data);
    }
}
