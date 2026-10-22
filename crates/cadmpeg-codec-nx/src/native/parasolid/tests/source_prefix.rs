// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::native::parasolid::parasolid_deltas_events_with_censuses;
use crate::parasolid::{ParasolidSubtype, Stream, StreamBody};
use crate::test_support::test_pmarks;

#[test]
fn partition_prefix_native_fields_and_exact_reads_come_from_the_original_nonzero_view() {
    let mut identifiers = Vec::new();
    for identifier in [0x1234_5678_u32, 0x8765_4321] {
        let mut payload = test_pmarks::prefix(1);
        let identifier_at = payload.len() - 4;
        payload[identifier_at..].copy_from_slice(&identifier.to_be_bytes());
        let mut bytes = b"outside NX".to_vec();
        let start = bytes.len();
        bytes.extend(&payload);
        bytes.extend(b"outside suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root.child(start, start + payload.len()).unwrap();
        let census = crate::deltas::census::walk(source.window()).unwrap();
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: payload.into(),
            source: Some(source),
            body: StreamBody::Parasolid {
                subtype: ParasolidSubtype::Deltas,
                schema: None,
            },
        }];
        let events =
            parasolid_deltas_events_with_censuses(&ctx, &streams, vec![Some(census)]).unwrap();
        assert_eq!(events.partition_prefixes.len(), 1);
        let prefix = &events.partition_prefixes[0];
        assert_eq!(prefix.state.highest_id(), u32::MAX);
        assert_eq!(prefix.state.current_pmark().identifier(), identifier);
        identifiers.push(prefix.state.current_pmark().identifier());
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        let capture = &captures[0];
        assert_eq!(capture.id(), prefix.id);
        assert_eq!(capture.stream(), "file");
        let offset = u64::try_from(start + test_pmarks::header().len()).unwrap();
        assert_eq!(capture.offset(), offset);
        assert_eq!(capture.byte_len(), 51);
        assert_eq!(
            capture.reads()["file"],
            [offset + 3..offset + 15, offset + 18..offset + 51]
        );
    }
    assert_eq!(identifiers, [0x1234_5678, 0x8765_4321]);
}

#[test]
fn discarded_prefix_native_creation_does_not_leave_reads_or_prevent_a_successful_peer() {
    let payload = test_pmarks::prefix(1);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: payload.clone().into(),
        source: Some(root),
        body: StreamBody::Parasolid {
            subtype: ParasolidSubtype::Deltas,
            schema: None,
        },
    }];
    let pending = ctx.read_transaction().unwrap();
    let discarded = parasolid_deltas_events_with_censuses(
        &ctx,
        &streams,
        vec![Some(crate::deltas::census::walk(&payload).unwrap())],
    )
    .unwrap();
    assert_eq!(discarded.partition_prefixes.len(), 1);
    drop(discarded);
    drop(pending);
    let kept = parasolid_deltas_events_with_censuses(
        &ctx,
        &streams,
        vec![Some(crate::deltas::census::walk(&payload).unwrap())],
    )
    .unwrap();
    assert_eq!(kept.partition_prefixes.len(), 1);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), kept.partition_prefixes[0].id);
    assert_eq!(captures[0].reads()["file"].len(), 2);
}
