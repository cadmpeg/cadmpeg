// SPDX-License-Identifier: Apache-2.0

use super::{compress, integers, wide_integers};
use crate::test_support::with_root;
use crate::usdc::compression::{expand, Size};
use crate::usdc::integers::{read, Width};

#[test]
fn a_buffer_expands_to_its_input() {
    for length in [
        0_usize, 1, 11, 12, 13, 14, 15, 16, 19, 269, 270, 271, 5000, 70_000,
    ] {
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

#[test]
fn a_repeating_buffer_expands_to_its_input_and_shrinks() {
    let mut data: Vec<u8> = Vec::new();
    for at in 0_u32..20_000 {
        data.extend_from_slice(&(at % 37).to_le_bytes());
    }
    data.extend(std::iter::repeat_n(7_u8, 600));
    let buffer = compress(&data).expect("compress");
    assert!(buffer.len() < data.len() / 10, "{}", buffer.len());
    with_root(&buffer, |ctx, view| {
        let expanded = expand(ctx, view, Size::Exact(data.len())).expect("expand");
        assert_eq!(expanded.window(), data.as_slice());
    });
}

#[test]
fn a_wide_integer_sequence_reads_back() {
    for values in [
        vec![],
        vec![i64::MIN, i64::MAX, 0, -1, 1 << 40, (1 << 40) + 1, 3],
        (0..100).map(|at| at * 70_000).collect::<Vec<i64>>(),
    ] {
        let mut out = Vec::new();
        wide_integers(&values, &mut out).expect("encode");
        with_root(&out, |ctx, root| {
            let mut view = root;
            let read = read(ctx, &mut view, values.len(), Width::Bits64).expect("read");
            assert_eq!(read, values);
        });
    }
}
