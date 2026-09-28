// SPDX-License-Identifier: Apache-2.0

use crate::om::counted_pattern_references::CountedPatternReferences;
use crate::om::operation_record::OperationPayload;

fn read_counted_pattern_test(record: OperationPayload<'_>) -> Option<CountedPatternReferences<()>> {
    crate::test_support::with_decode_context(|ctx| CountedPatternReferences::read(ctx, record)).unwrap()
}

#[test]
fn counted_pattern_references_refuse_collection_limit() {
    let mut payload = vec![1, 2, 0xf1, 0x06, 0xb1];
    payload.extend_from_slice(&[0, 0, 0, 0x37, 0xff, 0xff, 1, 0, 0, 0, 0x38, 0xff, 1, 0xff, 0xff, 0xff, 0xff, 1, 0xff]);
    let record = OperationPayload::new(&payload, 0, "Pattern Feature").unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let error = CountedPatternReferences::read(&ctx, record).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

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
    let lane = read_counted_pattern_test(record).expect("complete lane");
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
    assert!(read_counted_pattern_test(
        OperationPayload::new(&malformed, record.payload_offset(), record.name()).unwrap()
    )
    .is_none());
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(read_counted_pattern_test(
        OperationPayload::new(&ambiguous, record.payload_offset(), record.name()).unwrap()
    )
    .is_none());
}
