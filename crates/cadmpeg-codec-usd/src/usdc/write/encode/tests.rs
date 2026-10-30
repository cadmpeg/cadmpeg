// SPDX-License-Identifier: Apache-2.0

use super::{compress, integers};
use crate::test_support::with_root;
use crate::usdc::compression::{expand, Size};
use crate::usdc::integers::{read, Width};

#[test]
fn a_literal_buffer_expands_to_its_input() {
    for length in [0_usize, 1, 14, 15, 16, 269, 270, 271, 5000] {
        let data: Vec<u8> = (0..length)
            .map(|at| u8::try_from(at % 251).unwrap())
            .collect();
        let buffer = compress(&data).expect("compress");
        with_root(&buffer, |ctx, view| {
            let expanded = expand(ctx, view, Size::Exact(length)).expect("expand");
            assert_eq!(expanded.window(), data.as_slice(), "{length}");
        });
    }
}

#[test]
fn an_integer_sequence_reads_back() {
    for values in [
        vec![],
        vec![123, 124, 125, 100_125, 100_125, 100_126, 100_126],
        vec![-2, -1, 0, 7, i32::MIN, i32::MAX, 3],
        (0..100).collect::<Vec<i32>>(),
    ] {
        let mut out = Vec::new();
        integers(&values, &mut out).expect("encode");
        with_root(&out, |ctx, root| {
            let mut view = root;
            let read = read(ctx, &mut view, values.len(), Width::Bits32).expect("read");
            let expected: Vec<i64> = values.iter().map(|value| i64::from(*value)).collect();
            assert_eq!(read, expected);
        });
    }
}
