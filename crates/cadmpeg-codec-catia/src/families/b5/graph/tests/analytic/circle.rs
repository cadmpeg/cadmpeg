// SPDX-License-Identifier: Apache-2.0

use super::{evaluate_pcurve, parse_circle_pcurve, B5Record};

const CIRCLE_PCURVE_POINT_EPS: f64 = 1.0e-12;

#[test]
fn circle_pcurve_phase_does_not_rotate_the_native_chart() {
    for orientation in [-1.0_f64, 1.0] {
        for phase in [
            std::f64::consts::FRAC_PI_2,
            -3.75,
            2.0 * std::f64::consts::TAU,
        ] {
            let mut payload = vec![0x81, 0x18, 0x34, 0x12];
            for value in [3.0_f64, -4.0] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            payload.extend_from_slice(&[0x05, 0x05]);
            for value in [2.0, 0.0, std::f64::consts::PI, orientation, phase] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            let record = B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x19,
                object_id: 0x1235,
                payload,
            };
            let pcurve = parse_circle_pcurve(&record).expect("finite quarter-circle pcurve");
            assert_eq!(
                pcurve.distinct_knots,
                crate::test_support::test_b5::finite_lane(&[0.0, std::f64::consts::PI])
            );
            assert_eq!(pcurve.control_points.len(), 3);
            for (actual, expected) in pcurve.control_points.iter().zip([
                [5.0, -4.0],
                [5.0, -4.0 + 2.0 * orientation],
                [3.0, -4.0 + 2.0 * orientation],
            ]) {
                for (actual, expected) in actual.iter().zip(expected) {
                    assert!((actual - expected).abs() < CIRCLE_PCURVE_POINT_EPS,
                        "orientation={orientation}, phase={phase}, actual={actual}, expected={expected}");
                }
            }
            for (parameter, expected) in [
                (0.0, [5.0, -4.0]),
                (std::f64::consts::PI, [3.0, -4.0 + 2.0 * orientation]),
            ] {
                let actual = evaluate_pcurve(&pcurve, parameter).expect("quarter-circle endpoint");
                for (actual, expected) in actual.into_iter().zip(expected) {
                    assert!((actual - expected).abs() < CIRCLE_PCURVE_POINT_EPS);
                }
            }
        }
    }
}

#[test]
fn circle_pcurve_rejects_nonfinite_phase() {
    for phase in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut payload = vec![0x81, 0x18, 0x34, 0x12];
        for value in [3.0_f64, -4.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [2.0, 0.0, std::f64::consts::PI, 1.0, phase] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x19,
            object_id: 0x1235,
            payload,
        };
        assert!(parse_circle_pcurve(&record).is_none(), "phase={phase}");
    }
}
