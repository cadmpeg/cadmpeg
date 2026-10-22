// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::test_support::test_pmarks;

fn packet(identifier: u32) -> Vec<u8> {
    let mut bytes = vec![0, 1, 0, 1];
    bytes.extend(test_pmarks::node(40_000, 1, 40_001, 1));
    bytes.extend(test_pmarks::node(40_001, 40_000, 49_948, identifier));
    bytes.extend([0, 1, 0, 1, 0, 1, 0, 0, 0, 1]);
    bytes
}

#[test]
fn actual_compound_packet_reads_keep_all_nodes_and_exclude_tags_and_terminal() {
    for identifier in [2_u32, 7] {
        let payload = packet(identifier);
        let mut bytes = b"outside".to_vec();
        let start = bytes.len();
        bytes.extend(&payload);
        bytes.extend(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root.child(start, start + payload.len()).unwrap();
        let parsed = ctx
            .record("nx:test:packet#actual", source, || {
                super::read_source(&ctx, source, u32::MAX)
            })
            .unwrap();
        assert_eq!(parsed.nodes.as_slice().len(), 2);
        let nodes = parsed.nodes.as_slice();
        assert_eq!(nodes[0].xmt(), 40_000);
        assert_eq!(nodes[0].preceding(), 1);
        assert_eq!(nodes[0].first_following(), 40_001);
        assert_eq!(nodes[0].identifier(), 1);
        assert_eq!(nodes[1].xmt(), 40_001);
        assert_eq!(nodes[1].preceding(), 40_000);
        assert_eq!(nodes[1].first_following(), 49_948);
        assert_eq!(nodes[1].identifier(), identifier);
        assert!(parsed.terminal);
        assert_eq!(parsed.end, start + 90);
        let capture = ctx.take_record_captures().unwrap().pop().unwrap();
        let start = u64::try_from(start).unwrap();
        assert_eq!(capture.offset(), start);
        assert_eq!(capture.byte_len(), 90);
        assert_eq!(
            capture.reads()["file"],
            [start + 6..start + 41, start + 43..start + 80]
        );
    }
}

#[test]
fn a_late_invalid_node_rolls_back_earlier_reads_and_keeps_an_independent_packet() {
    let mut bad = packet(2);
    let logical = bad.len() - 10 - 5;
    bad[logical] = 2;
    let good = packet(7);
    let mut bytes = bad.clone();
    bytes.extend(&good);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let failed = root.child(0, bad.len()).unwrap();
    assert!(ctx
        .record("nx:test:packet#failed", failed, || {
            super::read_source(&ctx, failed, u32::MAX)
        })
        .is_err());
    let peer = root.child(bad.len(), bytes.len()).unwrap();
    let parsed = ctx
        .record("nx:test:packet#peer", peer, || {
            super::read_source(&ctx, peer, u32::MAX)
        })
        .unwrap();
    assert_eq!(parsed.nodes.as_slice()[1].identifier(), 7);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "nx:test:packet#peer");
    let start = u64::try_from(bad.len()).unwrap();
    assert_eq!(
        captures[0].reads()["file"],
        [start + 6..start + 41, start + 43..start + 80]
    );
}

#[test]
fn retained_packet_node_storage_uses_the_callers_remaining_session_budget() {
    let bytes = packet(2);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    ctx.retain_bytes(
        ctx.retained_bytes_available(),
        "exhaust retained packet budget",
    )
    .unwrap();
    let error = super::read_source(&ctx, root, u32::MAX).unwrap_err();
    let CodecError::ResourceLimit(limit) = &error else {
        panic!("real packet allocation budget");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "NX PMARK packet nodes");
    let later = ctx
        .retain_bytes(0, "later sibling after session refusal")
        .unwrap_err();
    let CodecError::ResourceLimit(later) = later else {
        panic!("same session refusal");
    };
    assert_eq!(&later, limit);
}
