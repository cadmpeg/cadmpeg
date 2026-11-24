// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use crate::wire::records::consolidated_records;

fn fixture() -> (Vec<u8>, Vec<Trim>) {
    let mut bytes = Vec::new();
    for control in [0x12, 0x1d, 0x0e, 0x1d] {
        let mut payload = vec![5, control];
        let scalars: &[f64] = match control {
            0x12 => &[0.2, 0.8],
            0x0e => &[3.0],
            _ => &[],
        };
        for scalar in scalars {
            payload.extend(scalar.to_le_bytes());
        }
        bytes.extend([
            0xb2,
            3,
            0x18,
            u8::try_from(payload.len()).expect("short line"),
            5,
        ]);
        bytes.extend(payload);
    }
    let ends = [
        [[0.2, 0.0], [0.8, 0.0]],
        [[0.8, -2.0], [0.8, 3.0]],
        [[0.8, 3.0], [0.2, 3.0]],
        [[0.2, 3.0], [0.2, -2.0]],
    ];
    let trims = ends
        .into_iter()
        .enumerate()
        .map(|(i, ends)| Trim {
            edge: i + 10,
            source: i,
            vertices: [i, (i + 1) % 4],
            pcurve: line_pcurve([0.0, 1.0], ends).expect("line"),
            range: [0.0, 1.0],
            mapped_range: None,
            affine_map: false,
            spline_range: None,
            stations: [Some(0.0), Some(1.0)],
            primary_stations: [None, None],
            sphere_circle: None,
            alternate_line: None,
            inferred_accuracy: None,
        })
        .collect();
    (bytes, trims)
}
fn recover(bytes: &[u8], trims: &mut [Trim]) -> Option<()> {
    let records = consolidated_records(bytes);
    cylinder_lower_trim(trims, [-2.0, 3.0], |trim| {
        compact_line_control(bytes, &records[trim.source])
    })
    .map(|(index, line)| {
        trims[index].pcurve = PcurveGeometry::Line(line);
    })
}
#[test]
fn cylinder_lower_trim_preserves_rotated_and_reversed_parameter_order() {
    for rotate in 0..4 {
        for reversed in [false, true] {
            let (bytes, mut trims) = fixture();
            if reversed {
                for trim in &mut trims {
                    let ends = [trim.uv(1.0).expect("end"), trim.uv(0.0).expect("start")];
                    trim.pcurve = line_pcurve([0.0, 1.0], ends).expect("reversed line");
                    trim.vertices.reverse();
                }
            }
            trims.rotate_left(rotate);
            recover(&bytes, &mut trims).expect("bounded cylinder ring");
            let lower = trims
                .iter()
                .find(|trim| trim.source == 0)
                .expect("lower side");
            let u = if reversed { [0.8, 0.2] } else { [0.2, 0.8] };
            for (t, expected_u) in [0.0, 1.0].into_iter().zip(u) {
                let [actual_u, v] = lower.uv(t).expect("lower endpoint");
                assert!((actual_u - expected_u).abs() <= 4.0 * f64::EPSILON);
                assert_eq!(v, -2.0);
            }
            assert_eq!(lower.range, [0.0, 1.0]);
            assert_eq!(lower.stations, [Some(0.0), Some(1.0)]);
        }
    }
}
#[test]
fn cylinder_lower_trim_rejects_contradictory_geometry_without_mutation() {
    for (index, ends) in [
        (1, [[0.8, -2.0], [0.7, 3.0]]),
        (1, [[0.8, -1.0], [0.8, 3.0]]),
        (2, [[0.8, 2.0], [0.2, 2.0]]),
        (0, [[0.2, 0.1], [0.8, 0.1]]),
    ] {
        let (bytes, mut trims) = fixture();
        trims[index].pcurve = line_pcurve([0.0, 1.0], ends).expect("changed side");
        let before = trims.iter().map(|t| t.pcurve.clone()).collect::<Vec<_>>();
        assert!(recover(&bytes, &mut trims).is_none());
        assert_eq!(
            trims.iter().map(|t| t.pcurve.clone()).collect::<Vec<_>>(),
            before
        );
    }
}
#[test]
fn cylinder_lower_trim_requires_the_complete_selector_pattern() {
    let (mut bytes, mut trims) = fixture();
    assert!(recover(&bytes, &mut trims[..3]).is_none());
    let records = consolidated_records(&bytes);
    for index in [1, 3] {
        let offset = records[index].payload().expect("side").start + 1;
        bytes[offset] = 0x1c;
    }
    assert!(recover(&bytes, &mut trims).is_none());
    let (mut bytes, mut trims) = fixture();
    let records = consolidated_records(&bytes);
    let offset = records[0].payload().expect("lower").start + 1;
    bytes[offset] = 0x13;
    assert!(recover(&bytes, &mut trims).is_none());
}

#[test]
fn cylinder_lower_trim_preserves_a_valid_interior_zero_locus() {
    let (bytes, mut trims) = fixture();
    for (i, endpoints) in [(1, [[0.8, 0.0], [0.8, 3.0]]), (3, [[0.2, 3.0], [0.2, 0.0]])] {
        trims[i].pcurve = line_pcurve([0.0, 1.0], endpoints).expect("interior side");
    }
    let before = trims
        .iter()
        .map(|trim| trim.pcurve.clone())
        .collect::<Vec<_>>();
    assert!(recover(&bytes, &mut trims).is_none());
    assert_eq!(
        trims
            .iter()
            .map(|trim| trim.pcurve.clone())
            .collect::<Vec<_>>(),
        before
    );
}
