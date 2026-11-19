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

#[test]
fn owned_surface_reports_local_limits_and_chart_maps_outside_the_neutral_carrier() {
    use crate::layout::surface_terminal_span_tail as layout;
    let cases = [
        (
            [3.0_f64, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0],
            vec![0x01, 0, 0x09, 0x01, 0x05, 0x07, 0x07],
            true,
        ),
        (
            [0.2, 0.8, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0],
            vec![0x04, 0x40, 0x01, 0x04, 0x41, 0x09, 0x00, 0x07, 0x07],
            false,
        ),
        (
            [0.2, 0.8, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0],
            vec![0x01, 0, 0x01, 0, 0x07, 0x07],
            false,
        ),
        (
            [0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0],
            vec![0x01, 0, 0x01, 0, 0x07, 0x07],
            false,
        ),
    ];
    for (parameters, suffix, descending) in cases {
        let mut tail = a5_surface_tail();
        if descending {
            tail[1] = 0x0d;
        }
        let values = parameters
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        tail[layout::PARAMETERS..layout::FLAGS].copy_from_slice(&values);
        tail.truncate(layout::SUFFIX);
        tail.extend(suffix);
        let bytes = a5_surface_stream_with_tail(&tail);
        let records = crate::wire::records::consolidated_records(&bytes);
        let mut refusal = crate::nurbs::LaneRefusals::new();
        let surfaces = super::super::a5_owned_surfaces_from_records(&bytes, &records, &mut refusal)
            .expect("owned surface decode");
        assert_eq!(surfaces.len(), 1);
        assert_eq!(
            surfaces[0].geometry.u_knots().as_slice(),
            &[0.0, 0.0, 1.0, 1.0]
        );
        assert_eq!(
            surfaces[0].geometry.v_knots().as_slice(),
            &[0.0, 0.0, 1.0, 1.0]
        );
        let notes = refusal.take_notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(
            notes[0].code.local_code(),
            "geometry.surface-parameters-not-transferred"
        );
        assert!(notes[0].message.contains("Surface record at offset 0"));
        assert!(notes[0]
            .message
            .contains(&format!("local U limits {:?}", &parameters[..2])));
        assert!(notes[0]
            .message
            .contains(&format!("affine map coefficients {:?}", &parameters[4..])));
    }
}

#[test]
fn owned_surface_keeps_full_domain_identity_parameters_without_a_loss() {
    let bytes = a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let mut refusal = crate::nurbs::LaneRefusals::new();
    let surfaces = super::super::a5_owned_surfaces_from_records(&bytes, &records, &mut refusal)
        .expect("full-domain surface");
    assert_eq!(surfaces.len(), 1);
    assert!(refusal.take_notes().is_empty());
}
