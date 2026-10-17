// SPDX-License-Identifier: Apache-2.0

use super::{
    group_member_id, GroupMemberTarget, ParasolidDeltasRecord, ParasolidDeltasTombstone,
    ParasolidGroupMember,
};
use crate::deltas::record_family::RecordFamily;
use crate::records::group::{MembershipKind, MembershipRecord};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct GroupState<'a> {
    records: BTreeMap<u32, Entry>,
    reservation: ScopedReservation<'a>,
}

enum Entry {
    Record(MembershipKind),
    Ambiguous,
}

impl<'a> GroupState<'a> {
    pub(super) fn from_records(
        ctx: &'a DecodeContext<'_>,
        records: &[MembershipRecord],
    ) -> Result<Self, CodecError> {
        let mut state = Self {
            records: BTreeMap::new(),
            reservation: ctx.reserve_scoped(0, "NX GROUP current record index")?,
        };
        for record in records {
            let xmt = u32::from(record.xmt);
            if let Some(existing) = state.records.get_mut(&xmt) {
                *existing = Entry::Ambiguous;
            } else {
                state.reserve_record(ctx)?;
                state.records.insert(xmt, Entry::Record(record.kind));
            }
        }
        Ok(state)
    }

    fn reserve_record(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.charge_collection_items(1, "NX GROUP current record index")?;
        self.reservation
            .grow(u64_from_index(std::mem::size_of::<(u32, Entry)>() * 4))
    }

    pub(super) fn apply_deltas(
        &mut self,
        ctx: &DecodeContext<'_>,
        stream_ordinal: u32,
        records: &[ParasolidDeltasRecord],
        tombstones: &[ParasolidDeltasTombstone],
    ) -> Result<(), CodecError> {
        enum Event<'a> {
            Record(&'a ParasolidDeltasRecord),
            Tombstone(u32),
        }
        let selection_work = records
            .len()
            .checked_add(tombstones.len())
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| ctx.refuse_codec_limit("NX GROUP state events", 0, 1))?;
        ctx.charge_work(
            u64_from_index(selection_work),
            "NX GROUP delta event selection",
        )?;
        let count = records
            .iter()
            .filter(|record| record.stream_ordinal == stream_ordinal)
            .count()
            .checked_add(
                tombstones
                    .iter()
                    .filter(|record| record.stream_ordinal == stream_ordinal)
                    .count(),
            )
            .ok_or_else(|| ctx.refuse_codec_limit("NX GROUP state events", 0, 1))?;
        let (mut events, _events_guard) = ctx.temporary_vec(count, "NX GROUP state events")?;
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
        ctx.stable_sort_by(
            &mut events,
            |(first, _), (second, _)| first.cmp(second),
            |_| 0,
            "sort NX GROUP state events",
        )?;
        for (_, event) in events {
            match event {
                Event::Record(record) => {
                    if matches!(self.records.get(&record.xmt), Some(Entry::Ambiguous)) {
                        continue;
                    }
                    if !self.records.contains_key(&record.xmt) {
                        self.reserve_record(ctx)?;
                    }
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
        Ok(())
    }

    pub(super) fn append_members(
        &self,
        ctx: &DecodeContext<'_>,
        stream_ordinal: u32,
        members: &mut Vec<ParasolidGroupMember>,
    ) -> Result<(), CodecError> {
        let unique_record = |xmt| match self.records.get(&xmt) {
            Some(Entry::Record(kind)) => Some(*kind),
            Some(Entry::Ambiguous) | None => None,
        };
        let mut groups_by_node =
            BTreeMap::<u32, Option<(u32, u32, crate::records::group::GroupSelector)>>::new();
        let mut group_guard = ctx.reserve_scoped(0, "NX GROUP node index")?;
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
                group_guard.grow(u64_from_index(
                    std::mem::size_of::<(
                        u32,
                        Option<(u32, u32, crate::records::group::GroupSelector)>,
                    )>() * 4,
                ))?;
                ctx.insert_btree_map(
                    &mut groups_by_node,
                    *node_id,
                    Some((xmt, *first_member, *selector)),
                    "NX GROUP node index",
                )?;
            }
        }
        for (&group_node_id, unique) in &groups_by_node {
            let Some((group_xmt, head, selector)) = *unique else {
                continue;
            };
            let mut chain = Vec::new();
            let mut chain_guard = ctx.reserve_scoped(0, "NX GROUP member chain")?;
            let mut seen = BTreeSet::new();
            let mut seen_guard = ctx.reserve_scoped(0, "NX GROUP member seen index")?;
            let mut current = head;
            let mut expected_previous = 1;
            let mut complete = true;
            while current != 1 {
                ctx.charge_work(1, "NX GROUP member chain")?;
                if seen.contains(&current) {
                    complete = false;
                    break;
                }
                seen_guard.grow(u64_from_index(std::mem::size_of::<u32>() * 4))?;
                ctx.insert_btree_set(&mut seen, current, "NX GROUP member seen index")?;
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
                ctx.reserve_scoped_vec(&mut chain_guard, &mut chain, 1, "NX GROUP member chain")?;
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
                ctx.reserve_vec(members, 1, "NX GROUP members")?;
                members.push(ParasolidGroupMember {
                    id: group_member_id(ctx, stream_ordinal, group_node_id, group_xmt, ordinal)?,
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
        Ok(())
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
