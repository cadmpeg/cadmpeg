// SPDX-License-Identifier: Apache-2.0
//! Complete selector forms and contradictory parameter relations.

use super::{parse, Class24Accuracy, Class24Selector, Class24SourceRelation, Class24TargetSites};

fn payload(selector: u16, lead: u8, values: &[f64]) -> Vec<u8> {
    let mut bytes = vec![0x82, 0x05, 0x09, lead];
    bytes.extend_from_slice(&selector.to_le_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn class24_observed_selectors_consume_both_accuracy_forms() {
    for selector in [
        0x0587_u16, 0x0586, 0x0585, 0x0584, 0x0987, 0x0985, 0x0984, 0x0d87, 0x0d86, 0x0d85, 0x0d84,
        1, 3,
    ] {
        for lead in [0x0a, 0x0b] {
            let signed = if lead == 0x0a { -0.25 } else { -0.0 };
            let mut values = vec![0.0, 10.0, signed];
            let family = if selector == 1 {
                5
            } else if selector == 3 {
                13
            } else {
                selector >> 8
            };
            match family {
                5 => (),
                9 => values.extend_from_slice(&[0.0, 10.0]),
                13 => values.extend_from_slice(&[0.0, 5.0, 2.0, 0.0]),
                _ => panic!("fixture family"),
            }
            let suffix = if matches!(selector, 1 | 3) {
                0x84
            } else {
                selector & 0xff
            };
            if matches!(suffix, 0x84 | 0x86) {
                values.push(0.0);
            }
            if matches!(suffix, 0x84 | 0x85) {
                values.push(10.0);
            }
            if lead == 0x0a {
                values.push(0.25);
            }
            let bytes = payload(selector, lead, &values);
            let relation = parse(&bytes).expect("observed complete production");
            assert_eq!(u16::from(relation.selector), selector);
            assert_eq!(relation.source_operand, 1);
            assert_eq!(relation.target_operand, 2);
            assert_eq!(relation.target_range.endpoints(), [0.0, 10.0]);
            assert!(match family {
                5 => matches!(relation.source, Class24SourceRelation::Absent),
                9 => matches!(relation.source, Class24SourceRelation::Identity { .. }),
                13 => matches!(relation.source, Class24SourceRelation::Affine { .. }),
                _ => false,
            });
            assert!(match suffix {
                0x87 => matches!(relation.sites, Class24TargetSites::Absent),
                0x86 => matches!(relation.sites, Class24TargetSites::Lower { .. }),
                0x85 => matches!(relation.sites, Class24TargetSites::Upper { .. }),
                0x84 => matches!(relation.sites, Class24TargetSites::Both { .. }),
                _ => false,
            });
            assert!(match lead {
                0x0a =>
                    matches!(relation.accuracy, Class24Accuracy::Repeated { signed } if signed.get() == -0.25),
                0x0b => matches!(
                    relation.accuracy,
                    Class24Accuracy::Elided {
                        negative_zero: true
                    }
                ),
                _ => false,
            });
            for extra in [vec![0], 0.0_f64.to_le_bytes().to_vec()] {
                let mut trailing = bytes.clone();
                trailing.extend_from_slice(&extra);
                assert!(parse(&trailing).is_none());
            }
            assert!(parse(&bytes[..bytes.len() - 1]).is_none());
        }
    }
    assert!(Class24Selector::try_from(0x0986).is_err());
    assert!(parse(&payload(
        0x0986,
        0x0a,
        &[0.0, 10.0, 0.0, 0.0, 10.0, 0.0, 0.0]
    ))
    .is_none());
    assert!(parse(&payload(0x0587, 0x0c, &[0.0, 10.0, 0.0])).is_none());
}

#[test]
fn class24_negative_scale_reverses_endpoint_pairing() {
    let values = [0.0, 10.0, -0.25, 0.0, 5.0, -2.0, 10.0, 0.25];
    let relation = parse(&payload(0x0d87, 0x0a, &values)).expect("negative scale map");
    assert!(
        matches!(relation.source, Class24SourceRelation::Affine { scale, .. } if scale.get() == -2.0)
    );
    for (index, replacement) in [
        (0, 10.0),
        (3, 5.0),
        (5, 0.0),
        (5, 2.0),
        (6, 0.0),
        (5, f64::NAN),
        (6, f64::INFINITY),
        (5, f64::MAX),
    ] {
        let mut invalid = values;
        invalid[index] = replacement;
        assert!(
            parse(&payload(0x0d87, 0x0a, &invalid)).is_none(),
            "lane {index}"
        );
    }
}

#[test]
fn class24_accuracy_closure_preserves_sign_and_exact_zero_words() {
    let repeated = [0.0, 10.0, -0.0, 0.0];
    assert!(
        matches!(parse(&payload(0x0587, 0x0a, &repeated)).expect("negative zero accuracy").accuracy,
        Class24Accuracy::Repeated { signed } if signed.get().is_sign_negative())
    );
    let mut wrong = repeated;
    wrong[3] = -0.0;
    assert!(parse(&payload(0x0587, 0x0a, &wrong)).is_none());
    assert!(parse(&payload(0x0587, 0x0b, &[0.0, 10.0, 1.0])).is_none());
    assert!(parse(&payload(0x0587, 0x0a, &[0.0, 10.0, -0.25, -0.25])).is_none());
}

#[test]
fn class24_sites_and_identity_use_derived_arithmetic_tolerance() {
    let adjacent_upper = f64::from_bits(10.0_f64.to_bits() + 1);
    assert!(parse(&payload(
        0x0585,
        0x0a,
        &[0.0, 10.0, 0.0, adjacent_upper, 0.0]
    ))
    .is_some());
    assert!(parse(&payload(0x0585, 0x0a, &[0.0, 10.0, 0.0, 11.0, 0.0])).is_none());
    assert!(parse(&payload(0x0584, 0x0a, &[0.0, 10.0, 0.0, 9.0, 1.0, 0.0])).is_none());
    assert!(parse(&payload(
        0x0987,
        0x0b,
        &[-1.0e7, 1.0e7, 0.0, -1.0e7 + 1.5e-7, 1.0e7]
    ))
    .is_some());
    assert!(parse(&payload(0x0987, 0x0b, &[0.0, 10.0, 0.0, 1.0, 10.0])).is_none());
}
