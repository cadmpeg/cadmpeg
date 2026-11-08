// SPDX-License-Identifier: Apache-2.0

use super::{b18_support, class25_representations};
use crate::families::consolidated::allocation::AllocationJournal;
use crate::families::consolidated::records::class25::{
    parse, Class25Operand, Class25SourceRelation,
};
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

fn frame(class: u8, increment: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        0x03,
        class,
        u8::try_from(payload.len()).expect("bounded test payload"),
        increment * 4 + 1,
    ];
    bytes.extend_from_slice(payload);
    bytes
}

fn line(operand: u8, range: [f64; 2]) -> Vec<u8> {
    let mut bytes = vec![operand * 4 + 1, 0x11];
    for value in range {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn map(first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x82];
    bytes.extend_from_slice(first);
    bytes.extend_from_slice(second);
    bytes.extend_from_slice(&[0x0a, 0x87, 0x0d]);
    for value in [20.0_f64, 40.0, 0.0, 2.0, 4.0, 10.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn class25_first_operand_selects_target_range_and_second_selects_source_range() {
    let mut data = vec![0xa5, 0x03, 0x34, 0, 0, 0, 0, 0x05];
    data.extend_from_slice(&frame(0x18, 2, &line(2, [20.0, 40.0])));
    data.extend_from_slice(&frame(0x18, 3, &line(5, [2.0, 4.0])));
    let payload = map(&[0x04, 7], &[0x04, 4]);
    data.extend_from_slice(&frame(0x25, 4, &payload));
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    let relation = parse(&payload).expect("complete affine relation");
    let representations = class25_representations(&journal, 3, &relation);
    assert_eq!(representations.target, Some(&records[1]));
    assert_eq!(representations.source, Some(&records[2]));
    assert_eq!(b18_support(&data, &journal, 1), Some(&records[0]));
    assert_eq!(b18_support(&data, &journal, 2), Some(&records[0]));
    assert!(relation
        .target_range
        .endpoints()
        .into_iter()
        .zip([20.0, 40.0])
        .all(|(a, b)| (a - b).abs() < 1e-12));
    let Class25SourceRelation::Affine { range, .. } = relation.source else {
        panic!("affine source")
    };
    assert!(range
        .endpoints()
        .into_iter()
        .zip([2.0, 4.0])
        .all(|(a, b)| (a - b).abs() < 1e-12));
}

#[test]
fn class25_owned_child_zero_selects_the_first_following_curve() {
    let payload = map(&[0x03], &[0x04, 1]);
    let data = [
        frame(0x18, 1, &line(0, [2.0, 4.0])),
        frame(0x25, 1, &payload),
        frame(0x18, 1, &line(0, [20.0, 40.0])),
    ]
    .concat();
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    let relation = parse(&payload).expect("complete owned-child relation");
    let representations = class25_representations(&journal, 1, &relation);
    assert_eq!(representations.target, Some(&records[2]));
    assert_eq!(representations.source, Some(&records[0]));
}

#[test]
fn class25_reference_forms_and_target_classes_remain_distinct() {
    let payload = map(&[0x04, 1], &[0x04, 1]);
    let data = [frame(0x18, 1, &[]), frame(0x25, 3, &payload)].concat();
    let records = consolidated_records(&data);
    let journal = AllocationJournal::new(&records);
    let mut relation = parse(&payload).expect("complete relation");
    relation.source_operand = Class25Operand {
        value: 1,
        encoding: AllocationReferenceEncoding::BackwardDistance,
    };
    let representations = class25_representations(&journal, 1, &relation);
    assert_eq!(representations.target, Some(&records[0]));
    assert!(representations.source.is_none());
    for encoding in [
        AllocationReferenceEncoding::Selector2,
        AllocationReferenceEncoding::TaggedU8,
        AllocationReferenceEncoding::TaggedU16,
    ] {
        relation.source_operand.encoding = encoding;
        assert!(class25_representations(&journal, 1, &relation)
            .target
            .is_none());
    }
    relation.source_operand = Class25Operand {
        value: 0,
        encoding: AllocationReferenceEncoding::WidthCoded,
    };
    assert!(class25_representations(&journal, 1, &relation)
        .target
        .is_none());
}
