// SPDX-License-Identifier: Apache-2.0
//! External block lengths retain body bounds when a body scalar is invalid.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn invalid_trailing_length_keeps_its_body_extent_and_the_next_exact_peer() {
    let metadata = super::super::synthetic_meta_table_body();
    let mut bulk = 0_u32.to_le_bytes().to_vec();
    bulk.extend([0x41; 3]);
    bulk.extend(99_u32.to_le_bytes());
    bulk.extend(0_u32.to_le_bytes());
    bulk.extend([0x42; 5]);
    bulk.extend(5_u32.to_le_bytes());
    bulk.extend(u32::MAX.to_le_bytes());
    let split = metadata.len();
    let mut source = metadata;
    source.extend(bulk);
    let arena = DecodeArena::new();
    let (_ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service())
        .expect("the source fits the service policy");
    let meta_source = root
        .child(0, split)
        .expect("the metadata lies in the source");
    let bulk_source = root
        .child(split, source.len())
        .expect("the bulk lies in the source");
    let tables = super::super::parse_meta_tables(meta_source).expect("the metadata tables parse");
    let error = super::super::frame_bulk_records(bulk_source, &tables, 18)
        .expect_err("an invalid trailing length is refused");
    assert!(error.to_string().contains("trails with 99"));
    let mut bodies = Vec::new();
    let framing =
        super::super::frame_bulk_sources(bulk_source, &tables, 18, |selector, block, source| {
            let start = source.body.start();
            let end = source.body.end();
            let native = source.into_typed(block, selector, tables.types[0]);
            bodies.push((start, end, native));
            Ok(())
        })
        .expect("the bulk sources frame");
    assert_eq!(framing.record_count, 2);
    assert_eq!(framing.stream_trailer.start(), split + 24);
    assert_eq!(framing.stream_trailer.window(), &u32::MAX.to_le_bytes());
    assert_eq!(bodies.len(), 2);
    assert_eq!((bodies[0].0, bodies[0].1), (split, split + 11));
    assert!(bodies[0]
        .2
        .as_ref()
        .expect_err("the first body keeps its trailing length error")
        .to_string()
        .contains("trails with 99"));
    assert_eq!((bodies[1].0, bodies[1].1), (split + 11, split + 24));
    let peer = bodies[1].2.as_ref().expect("the peer body is typed");
    assert_eq!(peer.ordinal, 1);
    assert_eq!(peer.type_id, [0x55; 16]);
    assert_eq!(peer.payload.window(), &[0x42; 5]);
    assert_eq!(
        peer.trailing_payload_len()
            .expect("the peer trailing length is valid"),
        5
    );
}
