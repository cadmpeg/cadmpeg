// SPDX-License-Identifier: Apache-2.0

use super::{
    GroupMemberTarget, ParasolidDeltasRecord, ParasolidDeltasTombstone, ParasolidGroupMember,
};
use crate::deltas::record_family::RecordFamily;
use crate::records::group::{MembershipKind, MembershipRecord};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct GroupState {
    records: BTreeMap<u32, Entry>,
}

enum Entry {
    Record(MembershipKind),
    Ambiguous,
}

impl GroupState {
    pub(super) fn from_records(records: &[MembershipRecord]) -> Self {
        let mut state = Self {
            records: BTreeMap::new(),
        };
        for record in records {
            let xmt = u32::from(record.xmt);
            if let Some(existing) = state.records.get_mut(&xmt) {
                *existing = Entry::Ambiguous;
            } else {
                state.records.insert(xmt, Entry::Record(record.kind));
            }
        }
        state
    }

    pub(super) fn apply_deltas(
        &mut self,
        stream_ordinal: u32,
        records: &[ParasolidDeltasRecord],
        tombstones: &[ParasolidDeltasTombstone],
    ) {
        enum Event<'a> {
            Record(&'a ParasolidDeltasRecord),
            Tombstone(u32),
        }
        let mut events = Vec::new();
        for record in records
            .iter()
            .filter(|record| record.stream_ordinal == stream_ordinal)
        {
            events.push((record.inflated_offset, Event::Record(record)));
        }
        for tombstone in tombstones
            .iter()
            .filter(|record| record.stream_ordinal == stream_ordinal)
        {
            events.push((tombstone.inflated_offset, Event::Tombstone(tombstone.xmt)));
        }
        events.sort_by_key(|(first, _)| *first);
        for (_, event) in events {
            match event {
                Event::Record(record) => {
                    if matches!(self.records.get(&record.xmt), Some(Entry::Ambiguous)) {
                        continue;
                    }
                    self.records.contains_key(&record.xmt);
                    self.records
                        .insert(record.xmt, Entry::Record(delta_kind(&record.family)));
                }
                Event::Tombstone(xmt) => {
                    if !matches!(self.records.get(&xmt), Some(Entry::Ambiguous)) {
                        self.records.remove(&xmt);
                    }
                }
            }
        }
    }

    pub(super) fn append_members(
        &self,
        stream_ordinal: u32,
        members: &mut Vec<ParasolidGroupMember>,
    ) {
        let unique_record = |xmt| match self.records.get(&xmt) {
            Some(Entry::Record(kind)) => Some(*kind),
            Some(Entry::Ambiguous) | None => None,
        };
        let mut groups_by_node =
            BTreeMap::<u32, Option<(u32, u32, crate::records::group::GroupSelector)>>::new();

        for (&xmt, kind) in &self.records {
            let Entry::Record(MembershipKind::Group {
                node_id,
                selector,
                first_member,
            }) = kind
            else {
                continue;
            };
            if let Some(unique) = groups_by_node.get_mut(node_id) {
                *unique = None;
            } else {
                groups_by_node.insert(*node_id, Some((xmt, *first_member, *selector)));
            }
        }
        for (&group_node_id, unique) in &groups_by_node {
            let Some((group_xmt, head, selector)) = *unique else {
                continue;
            };
            let mut chain = Vec::new();

            let mut seen = BTreeSet::new();

            let mut current = head;
            let mut expected_previous = 1;
            let mut complete = true;
            while current != 1 {
                if seen.contains(&current) {
                    complete = false;
                    break;
                }

                seen.insert(current);
                let Some(MembershipKind::Member {
                    owning_group,
                    owner,
                    next_member,
                    previous_member,
                }) = unique_record(current)
                else {
                    complete = false;
                    break;
                };
                if owning_group != group_xmt || previous_member != expected_previous {
                    complete = false;
                    break;
                }
                let Some(target) =
                    unique_record(owner).and_then(GroupMemberTarget::from_membership)
                else {
                    complete = false;
                    break;
                };
                if !target.matches_selector(selector) {
                    complete = false;
                    break;
                }
                chain.push((current, owner, target));
                expected_previous = current;
                current = next_member;
            }
            if !complete || chain.is_empty() {
                continue;
            }
            for (ordinal, (list_record_xmt, member_xmt, target)) in chain.into_iter().enumerate() {
                let Ok(ordinal_u32) = u32::try_from(ordinal) else {
                    continue;
                };
                members.push(ParasolidGroupMember {
                    id: format!("nx:s{stream_ordinal}:parasolid-group-member#{group_node_id}-{group_xmt}-{ordinal}"),
                    primary_stream_ordinal: stream_ordinal,
                    group_xmt,
                    group_node_id,
                    ordinal: ordinal_u32,
                    list_record_xmt,
                    member_xmt,
                    target,
                });
            }
        }
    }
}

fn delta_kind(record: &RecordFamily) -> MembershipKind {
    match record {
        RecordFamily::Group {
            node_id,
            selector,
            references,
            ..
        } => MembershipKind::Group {
            node_id: *node_id,
            selector: *selector,
            first_member: references[4],
        },
        RecordFamily::Type91 { references } => MembershipKind::Member {
            owning_group: references[0],
            owner: references[1],
            next_member: references[4],
            previous_member: references[5],
        },
        _ => record
            .node_id()
            .map_or(MembershipKind::Other, |node_id| MembershipKind::Node {
                kind: record.kind(),
                node_id,
            }),
    }
}
