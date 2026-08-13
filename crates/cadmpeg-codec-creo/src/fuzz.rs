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
}

/// Exercise Creo surface namespace row extraction.
pub fn surface_rows(data: &[u8]) {
    let _ = crate::surface::rows(data);
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

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::datum(&[]);
        super::curve_prototypes(&[]);
        super::surface_rows(&[]);
        super::scalar(&[]);
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::build_prt("1.0", &[]);
        super::datum(&data);
        super::curve_prototypes(&data);
        super::surface_rows(&data);
        super::scalar(&data);
    }
}
