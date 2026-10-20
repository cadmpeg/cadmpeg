// SPDX-License-Identifier: Apache-2.0
//! Complete map productions and contradictory counts, bounds, and accuracy.

use super::{parse, Class25Accuracy, Class25SamplePairs, Class25SourceRelation};
use crate::wire::bytes::AllocationReferenceEncoding;

fn payload(selector: u16, lead: u8, n: usize, decreasing: bool) -> Vec<u8> {
    let mut bytes = vec![0x82, 0x06, 0x8b, 0x0a, 0xc1, 0x01, lead];
    bytes.extend_from_slice(&selector.to_le_bytes());
    let signed = if lead == 0x0a { -0.25 } else { -0.0 };
    for value in [0.0_f64, 10.0, signed] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut words: Vec<f64> = Vec::new();
    match selector >> 8 {
        5 => (),
        13 => words.extend_from_slice(&[0.0, 5.0, -2.0, 10.0]),
        21 => {
            words.extend_from_slice(&[0.0, 5.0]);
            for value in &words {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            words.clear();
            bytes.push(0x80 + u8::try_from(n).expect("bounded fixture count"));
            for index in 0..n {
                words.push(
                    f64::from(u32::try_from(index).expect("small count")) * 8.0
                        / f64::from(u32::try_from(n - 1).expect("positive count"))
                        + 1.0,
                );
            }
            for index in 0..n {
                let index = if decreasing { n - 1 - index } else { index };
                words.push(
                    f64::from(u32::try_from(index).expect("small count")) * 3.0
                        / f64::from(u32::try_from(n - 1).expect("positive count"))
                        + 1.0,
                );
            }
        }
        _ => panic!("fixture selector"),
    }
    match selector & 0xff {
        0x86 => words.push(2.0),
        0x85 => words.push(8.0),
        0x84 => words.extend_from_slice(&[2.0, 8.0]),
        0x87 => (),
        _ => panic!("fixture sites"),
    }
    if lead == 0x0a {
        words.push(0.5);
    }
    for value in words {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn class25_selectors_preserve_addressing_forms_and_accuracy_encodings() {
    for selector in [
        0x0d87, 0x0d86, 0x0d85, 0x0d84, 0x0585, 0x1587, 0x1586, 0x1585, 0x1584,
    ] {
        for lead in [0x0a, 0x0b] {
            let bytes = payload(selector, lead, 2, true);
            let parsed = parse(&bytes);
            if selector >> 8 == 21 && lead == 0x0b {
                assert!(parsed.is_none());
                continue;
            }
            let map = parsed.expect("complete map");
            assert_eq!(u16::from(map.selector), selector);
            assert_eq!(map.source_operand.value, 139);
            assert_eq!(
                map.source_operand.encoding,
                AllocationReferenceEncoding::TaggedU8
            );
            assert_eq!(map.target_operand.value, 449);
            assert_eq!(
                map.target_operand.encoding,
                AllocationReferenceEncoding::TaggedU16
            );
            assert!(match lead {
                0x0a => matches!(map.accuracy, Class25Accuracy::Fitted { signed, fit_tolerance }
                    if signed.get() == -0.25 && fit_tolerance.get() == 0.5),
                _ => matches!(
                    map.accuracy,
                    Class25Accuracy::Elided {
                        negative_zero: true
                    }
                ),
            });
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(parse(&trailing).is_none());
            assert!(parse(&bytes[..bytes.len() - 1]).is_none());
        }
    }
    for selector in [0x0584_u16, 0x0586, 0x0587] {
        let mut bytes = payload(0x0585, 0x0a, 2, false);
        bytes[7..9].copy_from_slice(&selector.to_le_bytes());
        assert!(parse(&bytes).is_none());
    }
}

#[test]
fn class25_samples_keep_pairing_and_do_not_require_endpoint_samples() {
    for n in 2..=12 {
        for decreasing in [false, true] {
            let map = parse(&payload(0x1587, 0x0a, n, decreasing)).expect("bounded paired samples");
            let Class25SourceRelation::Sampled { samples, .. } = map.source else {
                panic!("sampled map")
            };
            let wire = serde_json::to_value(&samples).expect("sample arrays");
            assert_eq!(wire["target"].as_array().expect("target array").len(), n);
            assert_eq!(wire["target"][0], 1.0);
            assert_eq!(wire["target"][n - 1], 9.0);
            assert_eq!(wire["source"][0], if decreasing { 4.0 } else { 1.0 });
            assert_eq!(
                serde_json::from_value::<Class25SamplePairs>(wire).expect("load paired arrays"),
                samples
            );
        }
    }
    assert!(parse(&payload(0x1587, 0x0a, 13, false)).is_none());
    for wire in [
        serde_json::json!({"target":[1.0,2.0],"source":[1.0]}),
        serde_json::json!({"target":[2.0,1.0],"source":[1.0,2.0]}),
        serde_json::json!({"target":[1.0,2.0,3.0],"source":[1.0,3.0,2.0]}),
    ] {
        assert!(serde_json::from_value::<Class25SamplePairs>(wire).is_err());
    }
}

#[test]
fn class25_declines_wrong_count_bounds_fit_and_affine_equations() {
    let bytes = payload(0x1587, 0x0a, 2, true);
    for (offset, value) in [
        (9 + 40 + 1, 11.0_f64),
        (9 + 40 + 1 + 16, 6.0),
        (bytes.len() - 8, 0.1),
    ] {
        let mut wrong = bytes.clone();
        wrong[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(parse(&wrong).is_none());
    }
    let mut wrong = bytes;
    wrong[9 + 40] = 0x83;
    assert!(parse(&wrong).is_none());
    let affine = payload(0x0d84, 0x0a, 2, false);
    for (lane, value) in [
        (5, 0.0_f64),
        (5, 2.0),
        (6, 0.0),
        (7, 11.0),
        (8, 1.0),
        (5, f64::MAX),
        (6, f64::NAN),
    ] {
        let mut wrong = affine.clone();
        let at = 9 + lane * 8;
        wrong[at..at + 8].copy_from_slice(&value.to_le_bytes());
        assert!(parse(&wrong).is_none());
    }
    let mut elided = payload(0x0d87, 0x0b, 2, false);
    elided[9 + 16..9 + 24].copy_from_slice(&1.0_f64.to_le_bytes());
    assert!(parse(&elided).is_none());
}

#[test]
fn class25_named_sampled_witness_retains_two_arrays() {
    let frame = crate::test_support::test_class25::sampled_stream();
    let map = parse(&frame[5..]).expect("hand-parsed clothes-basket map");
    let Class25SourceRelation::Sampled { samples, .. } = map.source else {
        panic!("sampled map")
    };
    assert_eq!(
        serde_json::to_value(samples).expect("paired sample arrays"),
        serde_json::json!({"target":[0.0,18.232_007_915_496_25], "source":[0.0,18.232_007_915_496_25]})
    );
}
