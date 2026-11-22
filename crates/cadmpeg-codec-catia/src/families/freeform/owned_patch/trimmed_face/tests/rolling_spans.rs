// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::test_support::test_a5a8::a5_freeform_curve_stream;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

fn extended_stream(constant_radius: bool, mapped: bool) -> Vec<u8> {
    let mut bytes = a5_freeform_curve_stream();
    if constant_radius {
        let second_site = 8 + 4 + 2 * 8 + 80;
        for offset in [0, 4 * 8] {
            bytes[second_site + offset..second_site + offset + 8]
                .copy_from_slice(&1.0_f64.to_le_bytes());
        }
    }
    let tail = bytes.len() - 79;
    for offset in [24, 49] {
        bytes[tail + offset..tail + offset + 8].copy_from_slice(&4.0_f64.to_le_bytes());
    }
    if mapped {
        bytes.truncate(tail + 57);
        bytes.extend([1, 1]);
        for scalar in [0.0_f64, 1.0, 1.0, 1.0, 0.0, 0.0] {
            bytes.extend(scalar.to_le_bytes());
        }
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(0.0_f64.to_le_bytes());
        bytes.extend(1.0_f64.to_le_bytes());
        bytes.extend(4.0_f64.to_le_bytes());
    } else {
        bytes[tail + 57] = 1;
        bytes.extend(2.0_f64.to_le_bytes());
    }
    bytes.extend([0, 0]);
    let length = u32::try_from(bytes.len() - 8).expect("short jet");
    bytes[3..7].copy_from_slice(&length.to_le_bytes());
    for class in [0x2e, 0x27, 0x18, 0x18, 0x2e, 0x18] {
        bytes.extend([0xb2, 0x03, class, 1, 5, 0]);
    }
    let mut construction = vec![0x87, 29, 25, 21, 17, 13, 9, 5];
    construction.extend([0x11, 0x0d, 3, 5, 3]);
    construction.extend([0; 8]);
    construction.extend([1, 5]);
    bytes.extend([0xb2, 0x03, 0x37, 23, 5]);
    bytes.extend(construction);
    bytes
}

fn with_carrier<T>(bytes: &[u8], run: impl FnOnce(Carrier<'_>) -> T) -> Option<T> {
    let records = consolidated_records(bytes);
    let source = Source {
        bytes,
        records: &records,
        resolve: |index: usize, reference: AllocationReference| {
            (reference.encoding == AllocationReferenceEncoding::BackwardDistance)
                .then(|| index.checked_sub(usize::try_from(reference.value).ok()?))
                .flatten()
        },
        physical: Some,
        tensors: &BTreeMap::new(),
    };
    let ir = CadIr::empty();
    let carriers = BTreeMap::new();
    let mut refusal = LaneRefusals::new();
    source.carrier(&ir, 7, &carriers, &mut refusal).map(run)
}

#[test]
fn extended_rolling_spans_preserve_native_coordinates_and_local_radius() {
    for (constant, mapped) in [(false, false), (true, false), (false, true), (true, true)] {
        with_carrier(&extended_stream(constant, mapped), |carrier| {
            assert_eq!(carrier.bounds(), Some([[0.0, 1.0], [0.0, 4.0]]));
            for u in [0.0, 1.0] {
                let radius = if constant || u == 0.0 { 1.0 } else { 2.0 };
                for v in [0.0, 2.0, 3.0, 4.0] {
                    let angle = v * std::f64::consts::PI / if mapped { 8.0 } else { 4.0 };
                    let expected = Point3::new(radius * angle.cos(), radius * angle.sin(), 0.0);
                    assert!(
                        carrier
                            .evaluate(carrier.point([u, v]))
                            .expect("native span")
                            .distance(expected)
                            < 1e-9
                    );
                }
            }
            assert!(carrier.evaluate(carrier.point([0.5, 4.1])).is_none());
            let upper = if mapped { 1.0 } else { 2.0 };
            let surface = carrier
                .solved([[0.0, 1.0], [0.0, upper]], false)
                .expect("extended surface");
            let SolvedSurfaceGeometry::Nurbs(surface) = surface else {
                panic!("fitted tensor surface")
            };
            let radius = if constant { 1.0 } else { 1.5 };
            let angle = 3.0 * std::f64::consts::PI / if mapped { 8.0 } else { 4.0 };
            let expected = Point3::new(radius * angle.cos(), radius * angle.sin(), 0.0);
            let point = eval::decode::nurbs_surface_point(
                EvaluationAdmission::Standard,
                &surface,
                0.5,
                upper * 0.75,
            )
            .expect("fitted span");
            assert!(point.get().distance(expected) < 1e-3);
        })
        .expect("reference-closed extended construction");
    }
}

#[test]
fn extended_rolling_spans_require_resolved_roles_and_complete_controls() {
    let bytes = extended_stream(false, false);
    let records = consolidated_records(&bytes);
    let start = records[7].payload().expect("construction").start;
    for (offset, value) in [(1, 0xfd), (10, 2), (13, 1), (22, 9)] {
        let mut invalid = bytes.clone();
        invalid[start + offset] = value;
        assert!(with_carrier(&invalid, |_| ()).is_none());
    }
    let mut wrong_role = bytes.clone();
    wrong_role[records[1].range().expect("support").start + 2] = 0x18;
    assert!(with_carrier(&wrong_role, |_| ()).is_none());
    for length in [1, 8, 13, 22] {
        let mut truncated = bytes.clone();
        truncated.truncate(start + length);
        assert!(with_carrier(&truncated, |_| ()).is_none());
    }
}
