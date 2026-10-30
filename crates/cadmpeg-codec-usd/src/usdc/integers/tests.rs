// SPDX-License-Identifier: Apache-2.0

use super::{decode, read, Width};
use crate::test_support::with_root;

/// The coded form of the specification example (spec §9.4).
fn example_coded() -> Vec<u8> {
    let mut coded = 1_i32.to_le_bytes().to_vec();
    // Codes 1, 0, 0, 3 and 1, 0, 1.
    coded.extend_from_slice(&[0b1100_0001, 0b0001_0001]);
    coded.push(123);
    coded.extend_from_slice(&100_000_i32.to_le_bytes());
    coded.extend_from_slice(&[0, 0]);
    coded
}

const EXAMPLE: [i64; 7] = [123, 124, 125, 100_125, 100_125, 100_126, 100_126];

fn decoded(coded: &[u8], count: usize, width: Width) -> Result<Vec<i64>, String> {
    with_root(coded, |ctx, root| {
        decode(ctx, root, count, width).map_err(|error| error.to_string())
    })
}

#[test]
fn decodes_the_specification_example() {
    assert_eq!(
        decoded(&example_coded(), 7, Width::Bits32),
        Ok(EXAMPLE.to_vec())
    );
}

#[test]
fn wide_codes_use_wider_differences() {
    let mut coded = 0_i64.to_le_bytes().to_vec();
    // Codes 1, 2, 3.
    coded.push(0b0011_1001);
    coded.extend_from_slice(&(-2_i16).to_le_bytes());
    coded.extend_from_slice(&70_000_i32.to_le_bytes());
    coded.extend_from_slice(&(1_i64 << 40).to_le_bytes());
    assert_eq!(
        decoded(&coded, 3, Width::Bits64),
        Ok(vec![-2, 69_998, 69_998 + (1 << 40)])
    );
}

#[test]
fn differences_wrap_at_the_width() {
    let mut coded = 0_i32.to_le_bytes().to_vec();
    // Codes 3, 1.
    coded.push(0b0000_0111);
    coded.extend_from_slice(&i32::MAX.to_le_bytes());
    coded.push(1);
    assert_eq!(
        decoded(&coded, 2, Width::Bits32),
        Ok(vec![i64::from(i32::MAX), i64::from(i32::MIN)])
    );
}

#[test]
fn refuses_missing_differences() {
    let coded = example_coded();
    assert!(decoded(&coded[..coded.len() - 1], 7, Width::Bits32).is_err());
}

#[test]
fn reads_a_compressed_sequence() {
    let coded = example_coded();
    let mut input = u64::try_from(coded.len() + 2)
        .expect("size")
        .to_le_bytes()
        .to_vec();
    input.push(0);
    input.push(u8::try_from(coded.len()).expect("length") << 4);
    input.extend_from_slice(&coded);
    let values = with_root(&input, |ctx, root| {
        let mut view = root;
        read(ctx, &mut view, 7, Width::Bits32).map_err(|error| error.to_string())
    });
    assert_eq!(values, Ok(EXAMPLE.to_vec()));
}
