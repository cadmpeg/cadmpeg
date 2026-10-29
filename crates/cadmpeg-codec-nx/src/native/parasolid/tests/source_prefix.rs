// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::native::parasolid::parasolid_deltas_events_from_records;
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
        let (_ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root.child(start, start + payload.len()).unwrap();
        let owner = crate::records::DeltasRecords::read(source.window()).unwrap();
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
        let events = parasolid_deltas_events_from_records(&streams, vec![Some(owner)]).unwrap();
        assert_eq!(events.partition_prefixes.len(), 1);
        let state = serde_json::to_value(events.partition_prefixes[0].state).unwrap();
        assert_eq!(state["partition"]["highest_id"], u32::MAX);
        assert_eq!(state["current_pmark"]["identifier"], identifier);
        identifiers
            .push(u32::try_from(state["current_pmark"]["identifier"].as_u64().unwrap()).unwrap());
    }
    assert_eq!(identifiers, [0x1234_5678, 0x8765_4321]);
}
