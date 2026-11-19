// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rational_rows_combine_partial_mirrors_leading_repeats_and_row_copies() {
    let mut bytes = vec![0x05, 0x05, 0x00, 0x02, 0x00];
    bytes.extend([1.0, 0.7, 1.3, 0.9].into_iter().flat_map(le_f64));
    bytes.push(0x02);
    bytes.extend([0x01, 0x02, 0x00]);
    bytes.extend(
        [1.1, 0.8, 1.2, 0.6, 1.4, 0.95, 1.05]
            .into_iter()
            .flat_map(le_f64),
    );
    let program_end = bytes.len();
    bytes.extend([0xaa, 0xbb]);
    let mut at = 0;
    let weights = super::super::a5_weights(&bytes, &mut at, 3, 8, bytes.len())
        .expect("bounded rational row program");
    assert_eq!(at, program_end);
    let values = weights
        .into_iter()
        .map(cadmpeg_ir::scalar::NonZeroReal::get)
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            1.0, 1.0, 1.0, 0.7, 1.3, 1.0, 0.9, 1.0, 1.0, 1.0, 1.0, 0.7, 1.3, 1.0, 0.9, 1.0, 1.1,
            0.8, 1.2, 0.6, 1.4, 0.95, 0.8, 1.05,
        ]
    );
}

#[test]
fn rational_rows_retain_literal_middle_columns_and_unused_mirror_bits() {
    for mirror in [0x0003u16, 0xffff] {
        let mut bytes = vec![0x01];
        bytes.extend(mirror.to_le_bytes());
        bytes.extend([0.8, 1.2, 1.0].into_iter().flat_map(le_f64));
        let mut at = 0;
        let weights = super::super::a5_weights(&bytes, &mut at, 1, 5, bytes.len())
            .expect("bounded rational row program");
        assert_eq!(
            weights
                .into_iter()
                .map(cadmpeg_ir::scalar::NonZeroReal::get)
                .collect::<Vec<_>>(),
            vec![0.8, 1.2, 1.0, 1.2, 0.8]
        );
        assert_eq!(at, bytes.len());
    }
}

#[test]
fn rational_rows_bound_commands_repeats_and_literal_reads() {
    for bytes in [
        vec![0x02],
        vec![0x03],
        vec![0x05, 0, 0],
        vec![0x05, 0, 0, 4, 0],
    ] {
        assert!(super::super::a5_weights(&bytes, &mut 0, 1, 4, bytes.len()).is_none());
    }
    for value in [0.0, f64::NAN, f64::INFINITY] {
        let mut bytes = vec![0x05, 0, 0, 3, 0];
        bytes.extend(le_f64(value));
        assert!(super::super::a5_weights(&bytes, &mut 0, 1, 4, bytes.len()).is_none());
    }
    let mut bytes = vec![0x05, 0, 0, 3, 0];
    bytes.extend(le_f64(1.2));
    assert!(super::super::a5_weights(&bytes, &mut 0, 1, 4, bytes.len() - 1).is_none());
    assert!(super::super::a5_weights(&bytes, &mut 0, 1, 4, 3).is_none());
}

#[test]
fn rational_rows_preserve_legacy_symmetric_modes_for_even_and_odd_widths() {
    for mode in [3_u16, 7] {
        for columns in 1_u16..=40 {
            let cols = usize::from(columns);
            let seeds = (1..=columns.div_ceil(2)).map(f64::from).collect::<Vec<_>>();
            let mut bytes = vec![0x01];
            bytes.extend(mode.to_le_bytes());
            bytes.extend(seeds.iter().flat_map(|value| value.to_le_bytes()));
            let program_end = bytes.len();
            bytes.extend([0xaa, 0xbb]);
            let expected = seeds
                .iter()
                .copied()
                .chain(seeds[..cols / 2].iter().rev().copied())
                .collect::<Vec<_>>();
            let mut at = 0;
            let actual = super::super::a5_weights(&bytes, &mut at, 1, cols, bytes.len())
                .expect("complete legacy symmetric row");
            assert_eq!(
                actual
                    .into_iter()
                    .map(cadmpeg_ir::scalar::NonZeroReal::get)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(at, program_end, "mode {mode}, width {columns}");
            assert!(super::super::a5_weights(&bytes, &mut 0, 1, cols, program_end - 1).is_none());
        }
    }
}

#[test]
fn rational_rows_keep_legacy_mode_when_followed_by_a_copied_row() {
    let mut bytes = vec![0x01, 0x03, 0x00];
    bytes.extend([0.8, 1.2, 1.0].into_iter().flat_map(le_f64));
    bytes.push(0x02);
    let mut at = 0;
    let actual =
        super::super::a5_weights(&bytes, &mut at, 2, 6, bytes.len()).expect("legacy row and copy");
    assert_eq!(
        actual
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect::<Vec<_>>(),
        vec![0.8, 1.2, 1.0, 1.0, 1.2, 0.8, 0.8, 1.2, 1.0, 1.0, 1.2, 0.8]
    );
    assert_eq!(at, bytes.len());
}
