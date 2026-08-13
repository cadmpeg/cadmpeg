// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

/// Exercise Creo datum plane decoders.
pub fn datum(data: &[u8]) {
    let _ = crate::datum::planes(data);
    let _ = crate::datum::named_plane(data);
}

/// Exercise Creo curve prototype extraction.
pub fn curve_prototypes(data: &[u8]) {
    let _ = crate::curve::prototypes(data);
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::datum(&[]);
        super::curve_prototypes(&[]);
    }

    #[test]
    fn wrappers_accept_fixture() {
        let data = crate::test_support::build_prt("1.0", &[]);
        super::datum(&data);
        super::curve_prototypes(&data);
    }
}
