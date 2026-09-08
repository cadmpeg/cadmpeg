// SPDX-License-Identifier: Apache-2.0

use crate::om::counted_pattern_references::CountedPatternReferences;
use crate::om::operation_record::OperationPayload;

#[test]
fn om_pattern_counted_reference_lane_requires_exact_terminator() {
    const TRAILER: [u8; 19] = [
        0x00, 0x00, 0x00, 0x37, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0x38, 0xff, 0x01, 0xff, 0xff,
        0xff, 0xff, 0x01, 0xff,
    ];
    let references = [[0xf1, 0x06, 0xb1], [0xf1, 0x06, 0xb2], [0xf1, 0x06, 0xb3]];
    let mut payload = vec![0xaa, 0x01, references.len() as u8 + 1];
    for reference in &references {
        payload.extend_from_slice(reference);
    }
    payload.extend_from_slice(&TRAILER);
    let payload_offset = 200;
    let record = OperationPayload::new(&payload, payload_offset, "Pattern Feature").unwrap();
    let lane = CountedPatternReferences::read(record).expect("complete lane");
    assert_eq!(lane.offset(), (payload_offset + 1) as u64);
    assert_eq!(usize::from(lane.declared_count()), 4);
    assert_eq!(
        lane.iter()
            .map(|(_, token, ())| token.value())
            .collect::<Vec<_>>(),
        [0x06b1, 0x06b2, 0x06b3]
    );
    assert_eq!(
        lane.iter()
            .map(|(_, token, ())| token.raw().to_vec())
            .collect::<Vec<_>>(),
        references
            .iter()
            .map(|reference| reference.to_vec())
            .collect::<Vec<_>>()
    );

    let mut malformed = payload.clone();
    malformed.pop();
    assert!(CountedPatternReferences::read(
        OperationPayload::new(&malformed, record.payload_offset(), record.name()).unwrap()
    )
    .is_none());
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(CountedPatternReferences::read(
        OperationPayload::new(&ambiguous, record.payload_offset(), record.name()).unwrap()
    )
    .is_none());
}
