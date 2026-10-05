// SPDX-License-Identifier: Apache-2.0
use crate::native::parasolid::{group_member, ParasolidGroupMember};
use crate::test_support::test_prt::many_face_partition_stream;
use crate::topology::Graph;
use std::collections::BTreeMap;

use super::group_records::group_record;
use super::{record, stream};
use crate::native::parasolid::group_member::GroupMemberTarget;

#[test]
fn group_members_follow_complete_bidirectional_type_91_chain() {
    let group = record(90, 10, Some(7), vec![3, 4, 5, 6, 30]);
    let tail = record(91, 30, None, vec![10, 100, 3, 4, 20, 1]);
    let head = record(91, 20, None, vec![10, 101, 3, 4, 1, 30]);
    let tail_member = record(14, 100, Some(50), Vec::new());
    let head_member = record(16, 101, Some(51), Vec::new());
    let records = [group, tail, head, tail_member, head_member];
    let current = |records: &[crate::deltas::Record]| {
        crate::test_support::with_decode_context(|ctx| {
            let records: Vec<_> = records
                .iter()
                .map(|record| (record.xmt, &record.family))
                .collect();
            let mut members = Vec::new();
            crate::native::parasolid::group_members_from_records(ctx, 4, &records, &mut members)
                .unwrap();
            members
        })
    };

    let members = current(&records);

    assert_eq!(members.len(), 2);
    assert_eq!(members[0].list_record_xmt, 20);
    assert_eq!(
        serde_json::to_value(&members[0]).unwrap()["member_family"],
        "EDGE"
    );
    assert!(matches!(
        members[0].target,
        GroupMemberTarget::Node {
            node_id: 51,
            current_xmt: None,
            ..
        }
    ));
    assert_eq!(members[1].list_record_xmt, 30);
    assert_eq!(
        serde_json::to_value(&members[1]).unwrap()["member_family"],
        "FACE"
    );
    assert!(matches!(
        members[1].target,
        GroupMemberTarget::Node { node_id: 50, .. }
    ));

    let mut broken = records;
    broken[2].family = crate::deltas::record_family::RecordFamily::Type91 {
        references: [10, 101, 3, 4, 1, 99],
    };
    let broken_members = current(&broken);
    assert!(broken_members.is_empty());
}

#[test]
fn group_member_xmt_is_checked_before_node_identity_fallback() {
    let graph = crate::test_support::with_decode_context(|ctx| {
        Graph::parse(ctx, &many_face_partition_stream(1_000))
    })
    .unwrap();
    let resolve =
        |member: &ParasolidGroupMember| match member.target.resolve(&graph, member.member_xmt) {
            GroupMemberTarget::Fin => None,
            GroupMemberTarget::Node { current_xmt, .. } => current_xmt,
        };
    let member = ParasolidGroupMember {
        id: "member".into(),
        partition_stream_ordinal: 4,
        group_xmt: 10,
        group_node_id: 7,
        ordinal: 0,
        list_record_xmt: 20,
        member_xmt: 300,
        target: GroupMemberTarget::Node {
            family: group_member::GroupNodeFamily::Face,
            node_id: 1_000,
            current_xmt: None,
        },
    };

    assert_eq!(resolve(&member), Some(300));
    assert_eq!(
        resolve(&ParasolidGroupMember {
            member_xmt: 999,
            ..member.clone()
        },),
        Some(300)
    );
    assert_eq!(
        resolve(&ParasolidGroupMember {
            target: GroupMemberTarget::Node {
                family: group_member::GroupNodeFamily::Face,
                node_id: 2_000,
                current_xmt: None
            },
            ..member.clone()
        }),
        None
    );
}

#[test]
fn group_records_keep_equal_node_ids_in_distinct_partition_scopes() {
    let streams = [
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST",
            group_record(10, 7, 8),
        ),
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST",
            group_record(11, 7, 9),
        ),
    ];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);
    let groups = crate::test_support::with_decode_context(|ctx| {
        crate::native::parasolid::parasolid_groups(ctx, &streams, &BTreeMap::new(), &events)
    })
    .unwrap()
    .records;

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].node_id, groups[1].node_id);
    assert_eq!(groups[0].origin.partition_stream_ordinal(), Some(0));
    assert_eq!(groups[1].origin.partition_stream_ordinal(), Some(1));
    assert_eq!(u8::from(groups[0].selector), 4);
    assert_eq!(u8::from(groups[0].linked_reference_status), 0);
    assert_ne!(groups[0].id, groups[1].id);
}

#[test]
fn group_records_assign_only_paired_deltas_to_a_partition_scope() {
    let streams = [
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST",
            group_record(10, 7, 8),
        ),
        stream(
            crate::parasolid::ParasolidSubtype::Deltas,
            "SCH_TEST",
            group_record(11, 8, 9),
        ),
        stream(
            crate::parasolid::ParasolidSubtype::Deltas,
            "SCH_OTHER",
            group_record(12, 9, 10),
        ),
    ];
    let events = crate::native::parasolid::parasolid_deltas_events(&streams);
    let pairs = BTreeMap::from([(0, vec![1])]);

    let groups = crate::test_support::with_decode_context(|ctx| {
        crate::native::parasolid::parasolid_groups(ctx, &streams, &pairs, &events)
    })
    .unwrap()
    .records;

    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].origin.partition_stream_ordinal(), Some(0));
    assert_eq!(groups[1].origin.partition_stream_ordinal(), Some(0));
    assert_eq!(groups[2].origin.partition_stream_ordinal(), None);
    assert_eq!(groups[1].origin.stream_kind().label(), "deltas");
}

#[test]
fn group_members_replay_paired_deltas_events_in_offset_order() {
    use crate::native::parasolid::{ParasolidDeltasRecord, ParasolidDeltasTombstone};

    let streams = [
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST",
            group_record(10, 7, 30),
        ),
        stream(
            crate::parasolid::ParasolidSubtype::Deltas,
            "SCH_TEST",
            Vec::new(),
        ),
    ];
    let pairs = BTreeMap::from([(0, vec![1])]);
    let delta = |record: crate::deltas::Record, inflated_offset| ParasolidDeltasRecord {
        id: format!("nx:s1:deltas-record#{inflated_offset}-{}", record.xmt),
        stream_ordinal: 1,
        family: record.family,
        xmt: record.xmt,
        byte_len: 1,
        inflated_offset,
    };
    let members_with_tombstone_at = |tombstone_offset| {
        let mut events = crate::native::parasolid::parasolid_deltas_events(&[]);
        events.records = vec![
            delta(record(16, 101, Some(51), Vec::new()), 40),
            delta(record(91, 30, None, vec![10, 100, 3, 4, 20, 1]), 10),
            delta(record(14, 100, Some(50), Vec::new()), 30),
            delta(record(91, 20, None, vec![10, 101, 3, 4, 1, 30]), 20),
        ];
        events.tombstones = vec![ParasolidDeltasTombstone {
            id: "nx:s1:deltas-tombstone#0".into(),
            stream_ordinal: 1,
            kind: crate::deltas::record_kind::RecordKind::Edge,
            xmt: 101,
            inflated_offset: tombstone_offset,
        }];
        crate::test_support::with_decode_context(|ctx| {
            crate::native::parasolid::parasolid_groups(ctx, &streams, &pairs, &events)
        })
        .unwrap()
        .members
    };

    // A tombstone after the member record clears it and breaks the chain.
    assert!(members_with_tombstone_at(50).is_empty());
    // A record after the tombstone restores the member.
    let members = members_with_tombstone_at(35);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].list_record_xmt, 20);
    assert_eq!(members[1].member_xmt, 100);
}
