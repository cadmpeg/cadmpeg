// SPDX-License-Identifier: Apache-2.0

use super::super::DoubleXarSlot;
use super::{decode, framed};

const STOCK_ONE: u8 = 0x10;
const REPEAT: u8 = 0xe5;
const STOCK_ZERO: u8 = 0x0b;
const METRIC_SCALE: [u8; 4] = [0x07, 0x23, 0x11, 0x2e];
const CONTROL: [u8; 4] = [0xe8, 0x26, 0xd6, 0x95];
const SEVEN_BYTE_OPENERS: [u8; 21] = [
    0x31, 0x4a, 0x54, 0x6d, 0x73, 0x8a, 0x8b, 0x8c, 0x92, 0x93, 0x9a, 0x9b, 0x9c, 0x9f, 0xa3, 0xa5,
    0xa8, 0xbb, 0xbf, 0xd2, 0xdf,
];

#[derive(Clone, Copy)]
enum Scale {
    Metric,
    Imperial,
}

/// A model prefix, and the logical slot count it supplies.
fn model_prefix(leading_repeat: bool, scale: Scale, trailing_one: bool) -> (Vec<u8>, u8) {
    let mut bytes = vec![STOCK_ONE];
    let mut logical = 1;
    if leading_repeat {
        bytes.push(REPEAT);
        logical += 2;
    }
    match scale {
        Scale::Metric => bytes.extend(METRIC_SCALE),
        Scale::Imperial => {
            bytes.push(0x28);
            bytes.extend([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
            bytes.extend([0x23, 0x11, 0x2e]);
        }
    }
    bytes.push(STOCK_ZERO);
    bytes.extend(CONTROL);
    logical += 2;
    if trailing_one {
        bytes.push(STOCK_ONE);
        logical += 1;
    }
    (bytes, logical)
}

/// One bounding field of `width` source bytes, with a tail distinct per seed.
fn field(opener: u8, width: usize, seed: u8) -> Vec<u8> {
    let mut bytes = vec![opener];
    bytes.extend((1..width).map(|index| seed.wrapping_add(u8::try_from(index).expect("small"))));
    bytes
}

fn listed_width(opener: u8) -> usize {
    if matches!(opener, 0x2d | 0x41 | 0x46) {
        8
    } else {
        7
    }
}

fn table_with_openers(prefix: (Vec<u8>, u8), openers: [u8; 6]) -> (u8, Vec<u8>) {
    let (mut bytes, logical) = prefix;
    for (index, opener) in openers.into_iter().enumerate() {
        let seed = 0x20 + u8::try_from(index).expect("small") * 0x10;
        bytes.extend(field(opener, listed_width(opener), seed));
    }
    (logical + 6, bytes)
}

fn assert_complete_model_table(count: u8, body: &[u8], openers: [u8; 6]) {
    let tables = decode("Model_L05_PX", &framed(count, body));
    let [table] = tables.as_slice() else {
        panic!("one complete model table");
    };
    assert_eq!(table.logical_count, usize::from(count));
    assert_eq!(
        table
            .entries
            .iter()
            .map(DoubleXarSlot::logical_width)
            .sum::<usize>(),
        usize::from(count)
    );
    assert_eq!(
        table
            .entries
            .iter()
            .flat_map(DoubleXarSlot::raw)
            .copied()
            .collect::<Vec<_>>(),
        body
    );
    let bounds = table
        .entries
        .iter()
        .filter_map(|slot| match slot {
            DoubleXarSlot::ModelBound { value, raw } => Some((*value, raw.as_slice())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        bounds.iter().map(|(_, raw)| raw[0]).collect::<Vec<_>>(),
        openers
    );
    for (value, raw) in bounds {
        assert_eq!(raw.len(), listed_width(raw[0]));
        let high = match raw[0] {
            0x46 => Some(0x40),
            0x2d => Some(0xc0),
            _ => None,
        };
        let expected = high.map(|high| {
            let mut image = [high; 8];
            image[1..].copy_from_slice(&raw[1..]);
            f64::from_be_bytes(image)
        });
        assert_eq!(value, expected);
    }
}

#[test]
fn seven_byte_bounding_openers_complete_model_tables() {
    for (prefix, openers) in [
        (
            model_prefix(false, Scale::Metric, false),
            [0x46, 0xa8, 0x46, 0x2d, 0x2d, 0x2d],
        ),
        (
            model_prefix(true, Scale::Metric, false),
            [0x46, 0x46, 0xa5, 0x2d, 0x2d, 0x9f],
        ),
        (
            model_prefix(true, Scale::Imperial, false),
            [0x46, 0xdf, 0x41, 0xa3, 0x9c, 0x8a],
        ),
        (
            model_prefix(true, Scale::Metric, false),
            [0x4a, 0xbf, 0x46, 0x31, 0x2d, 0x2d],
        ),
        (
            model_prefix(true, Scale::Imperial, true),
            [0x46, 0x46, 0xdf, 0x2d, 0x2d, 0x9b],
        ),
    ] {
        let (count, body) = table_with_openers(prefix, openers);
        assert_complete_model_table(count, &body, openers);
    }
    for openers in SEVEN_BYTE_OPENERS.chunks_exact(6) {
        let openers = <[u8; 6]>::try_from(openers).expect("six openers");
        let (count, body) = table_with_openers(model_prefix(false, Scale::Metric, false), openers);
        assert_complete_model_table(count, &body, openers);
    }
    for opener in SEVEN_BYTE_OPENERS {
        let openers = [opener, 0x2d, 0x46, 0x41, 0x2d, 0x46];
        let (count, body) = table_with_openers(model_prefix(false, Scale::Metric, false), openers);
        assert_complete_model_table(count, &body, openers);
    }
}

#[test]
fn undecided_bounding_openers_leave_model_tables_unresolved() {
    for (opener, width) in [(0xb7, 7), (0xb7, 8), (0xda, 7), (0xda, 8), (0xe4, 1)] {
        let (prefix, logical) = model_prefix(true, Scale::Metric, false);
        let count = logical + 6;
        let table = |undecided: Vec<u8>| {
            let mut body = prefix.clone();
            body.extend(field(0x46, 8, 0x20));
            body.extend(undecided);
            for (index, listed) in [0x2d, 0x41, 0x2d, 0x46].into_iter().enumerate() {
                let seed = 0x40 + u8::try_from(index).expect("small") * 0x10;
                body.extend(field(listed, 8, seed));
            }
            body
        };
        assert_eq!(
            decode("Model_L05_PX", &framed(count, &table(field(0x31, 7, 0x30)))).len(),
            1
        );
        let undecided = table(field(opener, width, 0x30));
        assert!(decode("Model_L05_PX", &framed(count, &undecided)).is_empty());
    }
}
