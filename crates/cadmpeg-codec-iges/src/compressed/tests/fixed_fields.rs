// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn compressed_fixed_numbers_use_eight_column_decimal_fields() {
    for (value, expected) in [
        (0, *b"       0"),
        (1, *b"       1"),
        (-1, *b"      -1"),
        (99_999_999, *b"99999999"),
        (-9_999_999, *b"-9999999"),
    ] {
        assert_eq!(super::super::fixed_number(value).unwrap(), expected);
    }
    for value in [100_000_000, -10_000_000, i64::MIN, i64::MAX] {
        assert!(matches!(
            super::super::fixed_number(value),
            Err(CodecError::Malformed(message))
                if message == "IGES Directory field exceeds eight columns"
        ));
    }
}

#[test]
fn compressed_section_sequences_use_one_marker_and_seven_decimal_columns() {
    for marker in [b'S', b'G', b'D', b'P', b'T'] {
        for (sequence, digits) in [
            (0, *b"      0"),
            (1, *b"      1"),
            (9_999_999, *b"9999999"),
        ] {
            let field = super::super::sequence_field(marker, sequence).unwrap();
            assert_eq!(field[0], marker);
            assert_eq!(&field[1..], &digits);
        }
        for sequence in [10_000_000, u32::MAX] {
            assert!(matches!(
                super::super::sequence_field(marker, sequence),
                Err(CodecError::Malformed(message))
                    if message == "IGES Compressed ASCII: section sequence exceeds seven digits"
            ));
        }
    }
}
