// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn owned_surface_suffix_reads_independent_axis_continuations() {
    for suffix in [
        vec![0x0d, 0x00, 0x05, 0x00, 0x07, 0x07],
        vec![0x05, 0x01, 0x09, 0x05, 0x01, 0x05, 0x07, 0x07],
        vec![0x04, 0x40, 0x01, 0x04, 0x41, 0x09, 0x00, 0x07, 0x07],
    ] {
        let mut tail = a5_surface_tail();
        tail.truncate(crate::layout::surface_terminal_span_tail::SUFFIX);
        tail.extend(&suffix);
        assert_eq!(
            super::super::parse_surface_tail(&tail, 0, tail.len(), true),
            Some(tail.len())
        );
        tail.pop();
        assert_eq!(
            super::super::parse_surface_tail(&tail, 0, tail.len(), true),
            None
        );
    }
    for suffix in [
        vec![0x05, 0x02, 0x09, 0x05, 0x01, 0x05, 0x07, 0x07],
        vec![0x05, 0x01, 0x09, 0x05, 0x01, 0x04, 0x07, 0x07],
        vec![0x05, 0x00, 0x09, 0x00, 0x07, 0x07, 0x00],
    ] {
        assert!(super::super::owned_surface_suffix(&suffix).is_none());
    }
}

#[test]
fn owned_rational_surface_reads_repeated_rows_and_both_axis_continuations() {
    let mut bytes = a5_rational_surface_stream();
    let program = bytes
        .windows(3)
        .position(|part| part == [0x01, 0x07, 0x00])
        .expect("valid synthetic record");
    bytes.splice(program..program + 3, [0x05, 0x00, 0x00, 0x01, 0x00]);
    bytes.truncate(bytes.len() - 6);
    bytes.extend([0x05, 0x01, 0x09, 0x05, 0x01, 0x05, 0x07, 0x07]);
    let payload_len = u32::try_from(bytes.len() - 8).expect("valid synthetic record");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());
    let records = crate::wire::records::consolidated_records(&bytes);
    let surfaces = super::super::a5_owned_surfaces_from_records(
        &bytes,
        &records,
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("valid synthetic record");
    assert_eq!(surfaces.len(), 1);
    assert_eq!(
        surfaces[0]
            .geometry
            .pole_grid()
            .weights()
            .map(|rows| rows.concat()),
        Some(vec![2.0; 4])
    );
}
