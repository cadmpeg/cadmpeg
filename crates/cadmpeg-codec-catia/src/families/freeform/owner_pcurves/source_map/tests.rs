// SPDX-License-Identifier: Apache-2.0

use super::fitted_map;

const FIT: f64 = 1.0e-4;
const EPSILON: f64 = 1.0e-12;

fn payload(count: u8, selector: u16) -> Vec<u8> {
    let mut result = vec![0x82, 0x08, 8, 0, 0x08, 10, 0, 0x0a];
    result.extend(selector.to_le_bytes());
    for value in [0.0_f64, 1.0, 0.0, 2.4, 4.7] {
        result.extend(value.to_le_bytes());
    }
    result.push(0x80 + count);
    for (start, end) in [(0.0_f64, 1.0), (2.4, 4.7)] {
        for at in 0..count {
            let value = start + (end - start) * f64::from(at) / f64::from(count - 1);
            result.extend(value.to_le_bytes());
        }
    }
    match selector & 0xff {
        0x84 => {
            result.extend(0.2_f64.to_le_bytes());
            result.extend(0.8_f64.to_le_bytes());
        }
        0x85 | 0x86 => result.extend(0.5_f64.to_le_bytes()),
        _ => {}
    }
    result.extend(FIT.to_le_bytes());
    result
}

#[test]
fn complete_sampled_maps_admit_large_counts_and_all_selected_site_forms() {
    for count in [2, 15, 40, 127] {
        for selector in [0x1584, 0x1585, 0x1586, 0x1587] {
            let map = fitted_map(&payload(count, selector)).expect("complete fitted map");
            assert!((map.tolerance - FIT).abs() < EPSILON);
            assert!(map.ranges[1]
                .into_iter()
                .zip([2.4, 4.7])
                .all(|(actual, expected)| (actual - expected).abs() < EPSILON));
        }
    }
}

#[test]
fn malformed_counts_samples_sites_accuracy_and_trailers_are_withheld() {
    let original = payload(15, 0x1587);
    for case in 0..9 {
        let mut bytes = original.clone();
        let sample_start = bytes.len() - 8 - 15 * 16;
        match case {
            0 => bytes.push(0),
            1 => {
                bytes.pop();
            }
            2 => bytes[sample_start - 1] = 0x81,
            3 => bytes[sample_start - 1] = 0xff,
            4 => bytes[sample_start + 8..sample_start + 16].copy_from_slice(&0.0_f64.to_le_bytes()),
            5 => bytes[sample_start..sample_start + 8].copy_from_slice(&f64::NAN.to_le_bytes()),
            6 => {
                let source_start = sample_start + 15 * 8;
                bytes[source_start..source_start + 8].copy_from_slice(&1.0_f64.to_le_bytes());
            }
            7 => {
                let start = bytes.len() - 8;
                bytes[start..].copy_from_slice(&(-FIT).to_le_bytes());
            }
            _ => bytes[8..10].copy_from_slice(&0x1687_u16.to_le_bytes()),
        }
        assert!(fitted_map(&bytes).is_none(), "case {case}");
    }
    let mut sites = payload(15, 0x1584);
    let start = sites.len() - 24;
    sites[start..start + 8].copy_from_slice(&0.9_f64.to_le_bytes());
    assert!(fitted_map(&sites).is_none());
}

#[test]
fn fit_tolerance_resolves_b2_class_23_parameter_maps() {
    use super::{fit_tolerance, Selection};
    use crate::families::b2::records::B2EdgeNode;
    use crate::families::consolidated::allocation::AllocationJournal;
    use crate::wire::bytes::AllocationReferenceEncoding;
    use crate::wire::records::consolidated_records;

    // Build a synthetic consolidated stream:
    //   [0] A_20 pcurve representation (weight 1 -> token 0x05)
    //   [1] B_18 line representation   (weight 1 -> token 0x05)
    //   [2] B_23 source-map linking [0] and [1] via BackwardDistance 0x09 (2) and 0x05 (1)
    //   [3] B_60 edge node referencing [2] via BackwardDistance 0x05 (1)
    let mut map_payload = vec![0x82, 0x09, 0x05, 0x0a];
    map_payload.extend(0x1587_u16.to_le_bytes());
    for value in [0.0_f64, 1.0, 0.0, 2.4, 4.7] {
        map_payload.extend(value.to_le_bytes());
    }
    map_payload.push(0x82);
    for value in [0.0_f64, 1.0, 2.4, 4.7] {
        map_payload.extend(value.to_le_bytes());
    }
    map_payload.extend(FIT.to_le_bytes());

    let mut data = Vec::new();
    // [0] a5 03 20 len=0(u32) weight=5
    data.extend([0xa5, 0x03, 0x20, 0, 0, 0, 0, 0x05]);
    // [1] b2 03 18 len=0(u8) weight=5
    data.extend([0xb2, 0x03, 0x18, 0, 0x05]);
    // [2] b2 03 23 len=u8 weight=5 + map_payload
    let map_len = u8::try_from(map_payload.len()).expect("short map payload fits u8");
    data.extend([0xb2, 0x03, 0x23, map_len, 0x05]);
    data.extend(&map_payload);
    // [3] b2 03 60 len=0 weight=5
    data.extend([0xb2, 0x03, 0x60, 0, 0x05]);

    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    let node = B2EdgeNode {
        pos: 0,
        header_token: 0,
        curve_ref: 1,
        start_vertex_ref: 0,
        end_vertex_ref: 0,
        start_parameter_ref: 0,
        end_parameter_ref: 0,
        reference_encodings: [AllocationReferenceEncoding::BackwardDistance; 5],
        terminal_value: 0,
        terminal_encoding: AllocationReferenceEncoding::BackwardDistance,
        tail: 0,
    };
    let tol = fit_tolerance(
        &data,
        &journal,
        3,
        &node,
        Selection {
            representations: [0, 1],
            selected: 1,
            range: [2.4, 4.7],
        },
    )
    .expect("B_23 parameter map should resolve fit tolerance");
    assert!((tol - FIT).abs() < EPSILON);
}
