// SPDX-License-Identifier: Apache-2.0

use super::{PatternAtom, PatternNarrow, PatternScalarEncoding, PatternWide};

fn wide_atom(stated: f64) -> [u8; 8] {
    let mut raw = stated.to_be_bytes();
    raw[0] -= 0x10;
    raw
}

#[test]
fn a_four_byte_atom_is_a_binary64_prefix_stating_four_times_the_value() {
    let atom = PatternNarrow::read(&[0x50, 0x54, 0x00, 0x00]).unwrap();
    assert_eq!(atom.value().get(), 20.0);
    let negative = PatternNarrow::read(&[0xd0, 0x54, 0x00, 0x00]).unwrap();
    assert_eq!(negative.value().get(), -20.0);
    let read = PatternAtom::read(&[0x50, 0x54, 0x00, 0x00]).unwrap();
    assert_eq!(read.encoding(), PatternScalarEncoding::Binary32);
    assert_eq!(read.raw(), [0x50, 0x54, 0x00, 0x00]);
}

#[test]
fn an_eight_byte_atom_states_four_times_the_value() {
    let atom = PatternWide::read(&wide_atom(4.0 * 0.9595)).unwrap();
    assert!((atom.value().get() - 0.9595).abs() < 1.0e-15);
    let read = PatternAtom::read(&wide_atom(4.0 * 0.9595)).unwrap();
    assert_eq!(read.encoding(), PatternScalarEncoding::Binary64);
}

#[test]
fn rotation_rows_are_orthonormal_after_the_quarter_scale() {
    let angle = std::f64::consts::TAU / 22.0;
    let (sin, cos) = angle.sin_cos();
    let rows = [[cos, -sin], [sin, cos]].map(|row| {
        row.map(|entry| {
            PatternWide::read(&wide_atom(4.0 * entry))
                .unwrap()
                .value()
                .get()
        })
    });
    for (i, a) in rows.iter().enumerate() {
        for (j, b) in rows.iter().enumerate() {
            let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let expected = if i == j { 1.0 } else { 0.0 };
            assert!((dot - expected).abs() < 1.0e-12, "rows {i} and {j}: {dot}");
        }
    }
}
