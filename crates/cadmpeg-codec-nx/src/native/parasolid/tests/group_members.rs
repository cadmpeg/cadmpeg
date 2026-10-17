// SPDX-License-Identifier: Apache-2.0
use crate::native::parasolid::{group_member, ParasolidGroupMember};
use crate::test_support::test_prt::many_face_partition_stream;
use crate::topology::Graph;
use std::collections::BTreeMap;

use super::group_records::{group_record, primary_group_stream};
use super::{membership_record, stream};
use crate::native::parasolid::group_member::GroupMemberTarget;

#[test]
fn group_primary_scopes_include_plain_and_partition_sources() {
    let partition = primary_group_stream(10, 7, 30);
    let mut plain = partition.clone();
    let length =
        usize::try_from(cadmpeg_core::decode::View::u32_be_at(&plain, 2).unwrap()).unwrap();
    let description = b": TRANSMIT FILE created by the primary membership fixture";
    plain.splice(6..6 + length, description.iter().copied());
    plain[2..6].copy_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    let streams = [
        stream(
            crate::parasolid::ParasolidSubtype::Plain,
            "SCH_TEST_13006",
            plain,
        ),
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST_13006",
            partition,
        ),
    ];
    crate::test_support::with_decode_context(|ctx| {
        let groups = crate::native::parasolid::parasolid_groups(
            ctx,
            &streams,
            &BTreeMap::new(),
            &crate::native::parasolid::parasolid_deltas_events(&streams),
        )
        .unwrap()
        .0;
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups[0].origin.stream_kind(),
            crate::parasolid::StreamKind::Plain
        );
        assert_eq!(groups[0].origin.partition_stream_ordinal(), None);
        assert_eq!(groups[1].origin.partition_stream_ordinal(), Some(1));
        for mut source in streams {
            let mut bytes = std::mem::take(&mut source.inflated);
            bytes.truncate(bytes.len() - 4);
            bytes.extend_from_slice(&91u16.to_be_bytes());
            bytes.extend_from_slice(&30u16.to_be_bytes());
            bytes.extend_from_slice(&2u32.to_be_bytes());
            for pointer in [10u16, 100, 1, 1, 1, 1] {
                bytes.extend_from_slice(&pointer.to_be_bytes());
            }
            bytes.extend_from_slice(&16u16.to_be_bytes());
            bytes.extend_from_slice(&100u16.to_be_bytes());
            bytes.extend_from_slice(&50u32.to_be_bytes());
            bytes.extend_from_slice(&1u16.to_be_bytes());
            bytes.extend_from_slice(&0f64.to_be_bytes());
            for _ in 0..7 {
                bytes.extend_from_slice(&1u16.to_be_bytes());
            }
            bytes.extend_from_slice(&[0, 1, 0, 1]);
            source.inflated = bytes;
            let current_streams = [source];
            let events = crate::native::parasolid::parasolid_deltas_events(&current_streams);
            let (groups, members) = crate::native::parasolid::parasolid_groups(
                ctx,
                &current_streams,
                &BTreeMap::new(),
                &events,
            )
            .unwrap();
            assert_eq!(groups.len(), 1);
            assert_eq!(members.len(), 1);
            assert_eq!(members[0].primary_stream_ordinal, 0);
            assert_eq!(members[0].list_record_xmt, 30);
            assert_eq!(members[0].member_xmt, 100);
            assert_eq!(members[0].target.node_id, 50);
        }
    });
}

#[test]
fn group_members_follow_complete_bidirectional_type_91_chain() {
    let group = membership_record(90, 10, Some(7), vec![3, 4, 5, 6, 30]);
    let head = membership_record(91, 30, None, vec![10, 100, 3, 4, 20, 1]);
    let tail = membership_record(91, 20, None, vec![10, 101, 3, 4, 1, 30]);
    let head_member = membership_record(14, 100, Some(50), Vec::new());
    let tail_member = membership_record(16, 101, Some(51), Vec::new());
    let records = [group, head, tail, head_member, tail_member];

    let members = crate::test_support::with_decode_context(|ctx| {
        let mut members = Vec::new();
        crate::native::parasolid::group_state::GroupState::from_records(ctx, &records)
            .unwrap()
            .append_members(ctx, 4, &mut members)
            .unwrap();
        members
    });

    assert_eq!(members.len(), 2);
    assert_eq!(members[0].list_record_xmt, 30);
    assert_eq!(
        serde_json::to_value(&members[0]).unwrap()["member_family"],
        "FACE"
    );
    assert!(matches!(
        members[0].target,
        GroupMemberTarget {
            node_id: 50,
            current_xmt: None,
            ..
        }
    ));
    assert_eq!(members[1].list_record_xmt, 20);
    assert_eq!(
        serde_json::to_value(&members[1]).unwrap()["member_family"],
        "EDGE"
    );
    assert!(matches!(
        members[1].target,
        GroupMemberTarget { node_id: 51, .. }
    ));

    let mut broken = records;
    broken[2].kind = crate::records::group::MembershipKind::Member {
        owning_group: 10,
        owner: 101,
        next_member: 1,
        previous_member: 99,
    };
    let broken_members = crate::test_support::with_decode_context(|ctx| {
        let mut members = Vec::new();
        crate::native::parasolid::group_state::GroupState::from_records(ctx, &broken)
            .unwrap()
            .append_members(ctx, 4, &mut members)
            .unwrap();
        members
    });
    assert!(broken_members.is_empty());
}

#[test]
fn group_members_reuse_cached_delta_events_without_losing_order_or_ambiguity() {
    use crate::deltas::record_family::RecordFamily;
    use crate::deltas::record_kind::RecordKind;
    use crate::native::parasolid::{ParasolidDeltasRecord, ParasolidDeltasTombstone};
    let primary = [
        membership_record(90, 10, Some(7), vec![3, 4, 5, 6, 30]),
        membership_record(91, 30, None, vec![10, 100, 1, 1, 1, 1]),
        membership_record(14, 100, Some(50), Vec::new()),
    ];
    let replacement = |stream_ordinal, inflated_offset, node_id| ParasolidDeltasRecord {
        id: format!("replacement-{stream_ordinal}-{inflated_offset}"),
        stream_ordinal,
        family: RecordFamily::Face {
            references: [1; 11],
            node_id,
        },
        xmt: 100,
        byte_len: 1,
        inflated_offset,
    };
    let replacements = [
        replacement(1, 90, 51),
        replacement(1, 20, 52),
        replacement(2, 1000, 99),
    ];
    let tombstones = [ParasolidDeltasTombstone {
        id: "deletion".into(),
        stream_ordinal: 1,
        kind: RecordKind::Face,
        xmt: 100,
        inflated_offset: 50,
    }];
    crate::test_support::with_decode_context(|ctx| {
        let mut state =
            crate::native::parasolid::group_state::GroupState::from_records(ctx, &primary).unwrap();
        state
            .apply_deltas(ctx, 1, &replacements, &tombstones)
            .unwrap();
        let mut members = Vec::new();
        state.append_members(ctx, 4, &mut members).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].target.node_id, 51);
        let mut deleted =
            crate::native::parasolid::group_state::GroupState::from_records(ctx, &primary).unwrap();
        deleted
            .apply_deltas(ctx, 1, &replacements[1..2], &tombstones)
            .unwrap();
        let mut removed = Vec::new();
        deleted.append_members(ctx, 4, &mut removed).unwrap();
        assert!(removed.is_empty());
        let mut ambiguous = primary.to_vec();
        ambiguous.push(membership_record(14, 100, Some(53), Vec::new()));
        let mut state =
            crate::native::parasolid::group_state::GroupState::from_records(ctx, &ambiguous)
                .unwrap();
        state
            .apply_deltas(ctx, 1, &replacements, &tombstones)
            .unwrap();
        let mut rejected = Vec::new();
        state.append_members(ctx, 4, &mut rejected).unwrap();
        assert!(rejected.is_empty());
    });
}

#[test]
fn group_members_reject_ambiguous_owners_cycles_and_wrong_selectors() {
    use crate::records::group::{GroupSelector, MembershipKind};
    let records = [
        membership_record(90, 10, Some(7), vec![3, 4, 5, 6, 30]),
        membership_record(91, 30, None, vec![10, 100, 3, 4, 20, 1]),
        membership_record(91, 20, None, vec![10, 101, 3, 4, 1, 30]),
        membership_record(14, 100, Some(50), Vec::new()),
        membership_record(16, 101, Some(51), Vec::new()),
    ];
    for duplicate in [0, 1, 3] {
        let mut ambiguous = records.to_vec();
        ambiguous.push(records[duplicate]);
        let members = crate::test_support::with_decode_context(|ctx| {
            let state =
                crate::native::parasolid::group_state::GroupState::from_records(ctx, &ambiguous)
                    .unwrap();
            let mut members = Vec::new();
            state.append_members(ctx, 4, &mut members).unwrap();
            members
        });
        assert!(members.is_empty());
    }
    for replacement in [
        MembershipKind::Member {
            owning_group: 10,
            owner: 101,
            next_member: 30,
            previous_member: 30,
        },
        MembershipKind::Member {
            owning_group: 10,
            owner: 1,
            next_member: 1,
            previous_member: 30,
        },
        MembershipKind::Member {
            owning_group: 1,
            owner: 101,
            next_member: 1,
            previous_member: 30,
        },
        MembershipKind::Member {
            owning_group: 10,
            owner: 101,
            next_member: 99,
            previous_member: 30,
        },
    ] {
        let mut broken = records;
        broken[2].kind = replacement;
        let members = crate::test_support::with_decode_context(|ctx| {
            let state =
                crate::native::parasolid::group_state::GroupState::from_records(ctx, &broken)
                    .unwrap();
            let mut members = Vec::new();
            state.append_members(ctx, 4, &mut members).unwrap();
            members
        });
        assert!(members.is_empty());
    }
    let mut wrong_selector = records;
    wrong_selector[0].kind = MembershipKind::Group {
        node_id: 7,
        selector: GroupSelector::Edge,
        first_member: 30,
    };
    crate::test_support::with_decode_context(|ctx| {
        let state =
            crate::native::parasolid::group_state::GroupState::from_records(ctx, &wrong_selector)
                .unwrap();
        let mut members = Vec::new();
        state.append_members(ctx, 4, &mut members).unwrap();
        assert!(members.is_empty());
    });
}

#[test]
fn group_member_xmt_is_checked_before_node_identity_fallback() {
    let graph = crate::test_support::with_decode_context(|ctx| {
        Graph::parse(ctx, &many_face_partition_stream(1_000))
    })
    .unwrap();
    let resolve = |member: &ParasolidGroupMember| {
        member.target.resolve(&graph, member.member_xmt).current_xmt
    };
    let member = ParasolidGroupMember {
        id: "member".into(),
        primary_stream_ordinal: 4,
        group_xmt: 10,
        group_node_id: 7,
        ordinal: 0,
        list_record_xmt: 20,
        member_xmt: 300,
        target: GroupMemberTarget {
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
            target: GroupMemberTarget {
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
            "SCH_TEST_13006",
            primary_group_stream(10, 7, 8),
        ),
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST_13006",
            primary_group_stream(11, 7, 9),
        ),
    ];

    let groups = crate::test_support::with_decode_context(|ctx| {
        crate::native::parasolid::parasolid_groups(
            ctx,
            &streams,
            &BTreeMap::new(),
            &crate::native::parasolid::parasolid_deltas_events(&streams),
        )
    })
    .unwrap()
    .0;

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].node_id, groups[1].node_id);
    assert_eq!(groups[0].origin.partition_stream_ordinal(), Some(0));
    assert_eq!(groups[1].origin.partition_stream_ordinal(), Some(1));
    assert_eq!(u8::from(groups[0].selector), 4);
    assert!(serde_json::to_value(&groups[0])
        .unwrap()
        .get("linked_reference_status")
        .is_none());
    assert_ne!(groups[0].id, groups[1].id);
}

#[test]
fn group_records_assign_only_paired_deltas_to_a_partition_scope() {
    let streams = [
        stream(
            crate::parasolid::ParasolidSubtype::Partition,
            "SCH_TEST_13006",
            primary_group_stream(10, 7, 8),
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
    .0;

    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].origin.partition_stream_ordinal(), Some(0));
    assert_eq!(groups[1].origin.partition_stream_ordinal(), Some(0));
    assert_eq!(groups[2].origin.partition_stream_ordinal(), None);
    assert_eq!(groups[1].origin.stream_kind().label(), "deltas");
}
