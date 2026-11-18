// SPDX-License-Identifier: Apache-2.0

use super::{PatternAtom, PatternNarrow, PatternScalarEncoding, PatternTerminal, PatternWide};

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

#[test]
fn a_four_byte_wide_row_terminal_is_quarter_scaled_and_exact_one_is_one() {
    let narrow = PatternTerminal::read(&[0x50, 0x10, 0x00, 0x00]).unwrap();
    assert_eq!(narrow.encoding(), PatternScalarEncoding::Binary32);
    assert_eq!(narrow.value().get(), 1.0);
    let exact = PatternTerminal::read(&[0x01]).unwrap();
    assert_eq!(exact.encoding(), PatternScalarEncoding::ExactOne);
    assert_eq!(exact.value().get(), 1.0);
}

#[test]
fn the_first_byte_selects_the_atom_width() {
    let mut bytes = [0_u8; 8];
    bytes[1] = 0x40;
    for (head, width) in [
        (0x20, Some(8)),
        (0x3f, Some(8)),
        (0xa0, Some(8)),
        (0xbf, Some(8)),
        (0x40, Some(4)),
        (0x5f, Some(4)),
        (0xc0, Some(4)),
        (0xdf, Some(4)),
        (0x1f, None),
        (0x60, None),
        (0x9f, None),
        (0xe0, None),
    ] {
        bytes[0] = head;
        assert_eq!(
            PatternAtom::read(&bytes).map(|atom| atom.raw().len()),
            width,
            "{head:#04x}"
        );
    }
}
