// SPDX-License-Identifier: Apache-2.0
//! Parasolid source-record extractors and their record types.

use crate::framing::xmt_reference::XmtTarget;
use crate::parasolid::name_references::NameReferences;
use crate::parasolid::{Stream, StreamKind};
use crate::topology::blend_surface_state::BlendSurfaceState;
use crate::topology::offset_surface_state::OffsetSurfaceState;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::deltas::census::Census;
use crate::deltas::record_family::RecordFamily;
use crate::parasolid::attribute_action::AttributeAction;
use crate::parasolid::attribute_field::AttributeField;
use cadmpeg_ir::units::FiniteVector;
use std::fmt::Write;
use std::num::NonZeroU32;

pub(super) mod structured_value_kind;
use structured_value_kind::StructuredValueKind;

mod field_use_wire;
use field_use_wire::FieldUseWire;

pub(super) mod topology_attribute_kind;
use topology_attribute_kind::TopologyAttributeKind;

mod entity51_wire;
use crate::parasolid::counted_values::CountedValues;
use crate::parasolid::entity_references::{EntityReferences, FieldPosition};
use crate::parasolid::unicode_value::UnicodeValue;
use crate::printable_string::PrintableString;
use entity51_wire::Entity51Wire;
pub(super) mod named_fields;
use named_fields::NamedField;
mod body_revision_wire;
mod chart_wire;
mod support_uv_wire;
mod tail_wire;
mod transmit_header_wire;
use crate::deltas::inline_schema_fields::{InlineBodyStateFields, InlineSchemaFields};
use crate::deltas::packet_marker::ReferenceMarker;
use crate::deltas::preamble_state::PreambleState;
use crate::deltas::reference_lanes::{MapEntries, TaggedReferences};
use crate::deltas::state_frame::StateFrames;
use crate::deltas::tails::{NullTailForm, NumericTailValues};
use crate::deltas::transmit_state::TransmitState;
use crate::deltas::type150_state::Type150State;
use crate::framing::xmt_reference::NonNullXmt;
use body_revision_wire::RevisionLengths;
use transmit_header_wire::TransmitHeaderWire;
pub(super) mod group_member;
use group_member::GroupMemberTarget;
pub(super) mod group_record;
use crate::deltas::group::{GroupReferenceStatus, GroupSelector};
use group_record::GroupOrigin;

use super::substrate::{ParsedStreams, StreamView};

use std::collections::{BTreeMap, HashMap, HashSet};

/// One complete Parasolid GROUP record with its source and owning-partition scope.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "group_record::GroupWire")]
pub(super) struct ParasolidGroupRecord {
    /// Globally unique source-record identity.
    pub(super) id: String,
    /// Exact source stream and its partition namespace.
    pub(super) origin: GroupOrigin,
    /// Stream-local XMT identity.
    pub(super) xmt: u32,
    /// Partition-local kernel node identity.
    pub(super) node_id: u32,
    /// Ordered GROUP references without their framing status bytes.
    pub(super) references: [u32; 5],
    /// Selector between the four leading references and the linked reference.
    pub(super) selector: GroupSelector,
    /// Status byte following the linked reference.
    pub(super) linked_reference_status: GroupReferenceStatus,
    /// Exact serialized record length.
    pub(super) byte_len: u64,
    /// GROUP tag offset in the inflated source stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static GROUP_RECORD_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidGroupRecord {
    fn clone(&self) -> Self {
        GROUP_RECORD_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            origin: self.origin,
            xmt: self.xmt,
            node_id: self.node_id,
            references: self.references,
            selector: self.selector,
            linked_reference_status: self.linked_reference_status,
            byte_len: self.byte_len,
            inflated_offset: self.inflated_offset,
        }
    }
}

/// One topology member in a fully closed current Parasolid GROUP chain.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "group_member::MemberWire")]
pub(super) struct ParasolidGroupMember {
    /// Globally unique membership identity.
    pub(super) id: String,
    /// Partition whose local XMT and node namespaces own the chain.
    pub(super) partition_stream_ordinal: u32,
    /// Current GROUP record XMT identity.
    pub(super) group_xmt: u32,
    /// Current GROUP kernel node identity.
    pub(super) group_node_id: u32,
    /// Zero-based member order from the list head to tail.
    pub(super) ordinal: u32,
    /// `TYPE_91` list-record XMT identity.
    pub(super) list_record_xmt: u32,
    /// Member record XMT identity.
    pub(super) member_xmt: u32,
    /// Member family with its required node and optional current identity.
    pub(super) target: GroupMemberTarget,
}

#[cfg(test)]
std::thread_local! {
    static GROUP_MEMBER_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidGroupMember {
    fn clone(&self) -> Self {
        GROUP_MEMBER_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            partition_stream_ordinal: self.partition_stream_ordinal,
            group_xmt: self.group_xmt,
            group_node_id: self.group_node_id,
            ordinal: self.ordinal,
            list_record_xmt: self.list_record_xmt,
            member_xmt: self.member_xmt,
            target: self.target,
        }
    }
}

/// GROUP records and current GROUP membership of every partition stream.
pub(super) struct ParasolidGroups {
    pub(super) records: Vec<ParasolidGroupRecord>,
    pub(super) members: Vec<ParasolidGroupMember>,
}

/// One replayed GROUP state event at its inflated stream offset.
type GroupStateEvent<'a> = (u64, GroupStateChange<'a>);

/// A record that sets the current state of an identity, or a tombstone that clears it.
#[derive(Clone, Copy)]
enum GroupStateChange<'a> {
    Record { xmt: u32, family: &'a RecordFamily },
    Tombstone(u32),
}

/// Retain GROUP records and resolve current GROUP membership.
///
/// Each partition stream is walked once; its census yields the partition
/// GROUP records and the partition state that the paired deltas events then
/// update in stream order. Deltas records use the partition pairing already
/// selected for topology reconstruction. A record in an unpaired deltas stream
/// remains exact native evidence but has no partition-local namespace
/// assignment. Member targets are bound to partition graphs separately by
/// [`resolve_parasolid_group_member_targets`].
pub(super) fn parasolid_groups(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
    deltas: &ParasolidDeltasEvents,
) -> Result<ParasolidGroups, CodecError> {
    let mut paired_partition = HashMap::new();
    let mut pair_storage = ctx.reserve_scoped(0, "NX GROUP delta pair index")?;
    for (&partition, paired) in ctx.admit_iter(delta_pairs, "NX GROUP delta pair index")? {
        let Ok(partition) = u32::try_from(partition) else {
            continue;
        };
        for &delta in ctx.admit_iter(paired, "NX GROUP delta pair index")? {
            pair_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut paired_partition,
                    delta,
                    partition,
                    "NX GROUP delta pair index",
                )
            })?;
        }
    }
    let mut delta_events = BTreeMap::<u32, Vec<GroupStateEvent<'_>>>::new();
    let mut delta_event_storage = ctx.reserve_scoped(0, "NX GROUP deltas state events")?;
    for record in ctx.admit_iter(&deltas.records, "NX GROUP deltas state events")? {
        delta_event_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut delta_events,
                record.stream_ordinal,
                (
                    record.inflated_offset,
                    GroupStateChange::Record {
                        xmt: record.xmt,
                        family: &record.family,
                    },
                ),
                "NX GROUP deltas state events",
                "NX GROUP deltas state events",
            )
        })?;
    }
    for tombstone in ctx.admit_iter(&deltas.tombstones, "NX GROUP deltas state events")? {
        delta_event_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut delta_events,
                tombstone.stream_ordinal,
                (
                    tombstone.inflated_offset,
                    GroupStateChange::Tombstone(tombstone.xmt),
                ),
                "NX GROUP deltas state events",
                "NX GROUP deltas state events",
            )
        })?;
    }
    let mut groups = Vec::new();
    let mut members = Vec::new();
    for (stream_ordinal, stream) in ctx
        .admit_iter(streams, "NX GROUP partition streams")?
        .enumerate()
    {
        if stream.kind() != crate::parasolid::StreamKind::Partition {
            continue;
        }
        let Ok(stream_ordinal_u32) = u32::try_from(stream_ordinal) else {
            continue;
        };
        let census = crate::deltas::census::walk(ctx, &stream.inflated)?;
        for record in ctx.admit_iter(&census.records, "NX GROUP partition records")? {
            let RecordFamily::Group {
                node_id,
                selector,
                linked_reference_status,
                references,
            } = &record.family
            else {
                continue;
            };
            ctx.push_vec(
                &mut groups,
                ParasolidGroupRecord {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "parasolid-group",
                        record.offset,
                        Some(record.xmt),
                    )?,
                    origin: GroupOrigin::Partition {
                        stream_ordinal: stream_ordinal_u32,
                    },
                    xmt: record.xmt,
                    node_id: *node_id,
                    references: *references,
                    selector: *selector,
                    linked_reference_status: *linked_reference_status,
                    byte_len: cadmpeg_core::decode::u64_from_index(record.end - record.offset),
                    inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
                },
                "NX GROUP records",
            )?;
        }
        let mut current = BTreeMap::new();
        let mut current_storage = ctx.reserve_scoped(0, "NX GROUP current record index")?;
        let (partition_events, _partition_event_storage) = census_group_events(ctx, &census)?;
        apply_group_state_events(ctx, &mut current, &mut current_storage, &partition_events)?;
        let mut complete = true;
        if let Some(paired) =
            ctx.get_btree_map(delta_pairs, &stream_ordinal, "NX GROUP delta pair index")?
        {
            for &delta in ctx.admit_iter(paired, "NX GROUP delta pair index")? {
                if delta >= streams.len() {
                    complete = false;
                    break;
                }
                let Ok(delta) = u32::try_from(delta) else {
                    complete = false;
                    break;
                };
                let Some(events) =
                    ctx.get_btree_map(&delta_events, &delta, "NX GROUP deltas state events")?
                else {
                    continue;
                };
                let (mut ordered, _ordered_storage) =
                    ctx.copy_temporary_slice(events, "NX GROUP deltas state events")?;
                ctx.stable_sort_by(
                    &mut ordered,
                    |event| &event.0,
                    Ord::cmp,
                    "sort NX GROUP state events",
                )?;
                apply_group_state_events(ctx, &mut current, &mut current_storage, &ordered)?;
            }
        }
        if !complete {
            continue;
        }
        let (mut records, mut records_storage) =
            ctx.scoped_vector_storage(current.len(), "NX GROUP current records")?;
        for (&xmt, &family) in ctx.admit_iter(&current, "NX GROUP current records")? {
            ctx.reserve_scoped_vec(
                &mut records_storage,
                &mut records,
                1,
                "NX GROUP current records",
            )?;
            records.push((xmt, family));
        }
        group_members_from_records(ctx, stream_ordinal_u32, &records, &mut members)?;
    }
    for record in ctx.admit_iter(&deltas.records, "NX GROUP deltas records")? {
        let RecordFamily::Group {
            node_id,
            selector,
            linked_reference_status,
            references,
        } = &record.family
        else {
            continue;
        };
        let partition_stream_ordinal = match usize::try_from(record.stream_ordinal) {
            Ok(delta) => ctx
                .get_hash_map(&paired_partition, &delta, "NX GROUP delta pair index")?
                .copied(),
            Err(_) => None,
        };
        ctx.push_vec(
            &mut groups,
            ParasolidGroupRecord {
                id: replace_group_record_id(ctx, &record.id)?,
                origin: GroupOrigin::Deltas {
                    stream_ordinal: record.stream_ordinal,
                    partition_stream_ordinal,
                },
                xmt: record.xmt,
                node_id: *node_id,
                references: *references,
                selector: *selector,
                linked_reference_status: *linked_reference_status,
                byte_len: record.byte_len,
                inflated_offset: record.inflated_offset,
            },
            "NX GROUP records",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut groups,
        |value| (value.origin.stream_ordinal(), value.inflated_offset),
        Ord::cmp,
        "sort NX GROUP records",
    )?;
    Ok(ParasolidGroups {
        records: groups,
        members,
    })
}

/// A malformed-record error whose message is copied under the decode budget.
fn retained_malformed(ctx: &DecodeContext<'_>, message: &str) -> CodecError {
    match ctx.copy_retained_text(message, "retain NX Parasolid rejection") {
        Ok(message) => CodecError::Malformed(message),
        Err(error) => error,
    }
}

fn replace_group_record_id(ctx: &DecodeContext<'_>, id: &str) -> Result<String, CodecError> {
    let operation = "NX GROUP record identity";
    match ctx.split_once(id, "deltas-record", operation)? {
        Some((prefix, suffix)) => {
            ctx.format_retained(format_args!("{prefix}parasolid-group{suffix}"), operation)
        }
        None => ctx.copy_retained_text(id, operation),
    }
}

/// Members of every complete GROUP chain among one partition's current records.
fn group_members_from_records(
    ctx: &DecodeContext<'_>,
    partition_stream_ordinal: u32,
    records: &[(u32, &RecordFamily)],
    members: &mut Vec<ParasolidGroupMember>,
) -> Result<(), CodecError> {
    let (records_by_xmt, _record_storage) =
        ctx.unique_index(records.iter().copied(), "NX GROUP record index")?;
    let unique_record = |xmt: u32| -> Result<Option<&RecordFamily>, CodecError> {
        Ok(ctx
            .get_hash_map(&records_by_xmt, &xmt, "NX GROUP record index")?
            .copied()
            .flatten())
    };
    let mut groups_by_node = BTreeMap::<u32, Option<(u32, u32)>>::new();
    let mut group_storage = ctx.reserve_scoped(0, "NX GROUP node index")?;
    for &(xmt, family) in ctx.admit_iter(records, "NX GROUP node index")? {
        if let RecordFamily::Group {
            node_id,
            references,
            ..
        } = family
        {
            if let Some(unique) =
                ctx.get_mut_btree_map(&mut groups_by_node, node_id, "NX GROUP node index")?
            {
                *unique = None;
            } else {
                group_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut groups_by_node,
                        *node_id,
                        Some((xmt, references[4])),
                        "NX GROUP node index",
                    )
                })?;
            }
        }
    }
    for (&group_node_id, unique) in ctx.admit_iter(&groups_by_node, "NX GROUP member chains")? {
        let Some((group_xmt, tail)) = *unique else {
            continue;
        };
        let mut reverse_chain = Vec::new();
        let mut chain_storage = ctx.reserve_scoped(0, "NX GROUP member chain")?;
        let mut seen = HashSet::new();
        let mut seen_storage = ctx.reserve_scoped(0, "NX GROUP member seen index")?;
        let mut current = tail;
        let mut expected_next = 1;
        let mut complete = true;
        while current != 1 {
            ctx.charge_work(1, "NX GROUP member chain")?;
            if ctx.contains_hash_set(&seen, &current, "NX GROUP member seen index")? {
                complete = false;
                break;
            }
            seen_storage.with_storage(|| {
                ctx.insert_hash_set(&mut seen, current, "NX GROUP member seen index")
            })?;
            let Some(RecordFamily::Type91 { references }) = unique_record(current)? else {
                complete = false;
                break;
            };
            if references[0] != group_xmt || references[5] != expected_next {
                complete = false;
                break;
            }
            let member_xmt = references[1];
            let Some(member_family) = unique_record(member_xmt)? else {
                complete = false;
                break;
            };
            let Some(target) = GroupMemberTarget::from_record(member_family) else {
                complete = false;
                break;
            };
            ctx.push_scoped_vec(
                &mut chain_storage,
                &mut reverse_chain,
                (current, member_xmt, target),
                "NX GROUP member chain",
            )?;
            expected_next = current;
            current = references[4];
        }
        if !complete || reverse_chain.is_empty() {
            continue;
        }
        ctx.reverse(&mut reverse_chain, "NX GROUP member chain")?;
        for (ordinal, (list_record_xmt, member_xmt, target)) in ctx
            .admit_iter(reverse_chain, "NX GROUP members")?
            .enumerate()
        {
            let Ok(ordinal_u32) = u32::try_from(ordinal) else {
                continue;
            };
            ctx.reserve_vec(members, 1, "NX GROUP members")?;
            members.push(ParasolidGroupMember {
                id: group_member_id(
                    ctx,
                    partition_stream_ordinal,
                    group_node_id,
                    group_xmt,
                    ordinal,
                )?,
                partition_stream_ordinal,
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

fn group_member_id(
    ctx: &DecodeContext<'_>,
    partition_stream_ordinal: u32,
    group_node_id: u32,
    group_xmt: u32,
    ordinal: usize,
) -> Result<String, CodecError> {
    let digits = |value: u64| {
        value
            .checked_ilog10()
            .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
    };
    let length = "nx:s"
        .len()
        .checked_add(digits(u64::from(partition_stream_ordinal)))
        .and_then(|length| length.checked_add(":parasolid-group-member#".len()))
        .and_then(|length| length.checked_add(digits(u64::from(group_node_id))))
        .and_then(|length| length.checked_add(1 + digits(u64::from(group_xmt))))
        .and_then(|length| {
            length.checked_add(1 + digits(cadmpeg_core::decode::u64_from_index(ordinal)))
        })
        .ok_or_else(|| ctx.refuse_codec_limit("NX GROUP member identity", 0, 1))?;
    let mut id = ctx.retained_string(length, "NX GROUP member identity")?;
    write!(&mut id, "nx:s{partition_stream_ordinal}:parasolid-group-member#{group_node_id}-{group_xmt}-{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit("write NX GROUP member identity", 0, 1))?;
    Ok(id)
}

/// Orders one partition census's records and tombstones by stream offset.
fn census_group_events<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    census: &'a Census,
) -> Result<
    (
        Vec<GroupStateEvent<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let count = census
        .records
        .len()
        .checked_add(census.tombstones.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX GROUP state events", 0, 1))?;
    let (mut events, mut storage) = ctx.scoped_vector_storage(count, "NX GROUP state events")?;
    for record in ctx.admit_iter(&census.records, "NX GROUP state events")? {
        ctx.reserve_scoped_vec(&mut storage, &mut events, 1, "NX GROUP state events")?;
        events.push((
            cadmpeg_core::decode::u64_from_index(record.offset),
            GroupStateChange::Record {
                xmt: record.xmt,
                family: &record.family,
            },
        ));
    }
    for tombstone in ctx.admit_iter(&census.tombstones, "NX GROUP state events")? {
        ctx.reserve_scoped_vec(&mut storage, &mut events, 1, "NX GROUP state events")?;
        events.push((
            cadmpeg_core::decode::u64_from_index(tombstone.offset),
            GroupStateChange::Tombstone(tombstone.xmt),
        ));
    }
    ctx.stable_sort_by(
        &mut events,
        |event| &event.0,
        Ord::cmp,
        "sort NX GROUP state events",
    )?;
    Ok((events, storage))
}

/// Replays ordered state events onto the current record of each identity.
fn apply_group_state_events<'a>(
    ctx: &DecodeContext<'_>,
    current: &mut BTreeMap<u32, &'a RecordFamily>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    events: &[GroupStateEvent<'a>],
) -> Result<(), CodecError> {
    for &(_, change) in ctx.admit_iter(events, "NX GROUP current record index")? {
        match change {
            GroupStateChange::Record { xmt, family } => {
                storage.with_storage(|| {
                    ctx.insert_btree_map(current, xmt, family, "NX GROUP current record index")
                })?;
            }
            GroupStateChange::Tombstone(xmt) => {
                ctx.remove_btree_map(current, &xmt, "NX GROUP current record index")?;
            }
        }
    }
    Ok(())
}

/// Bind each GROUP member target to the current topology of its partition.
pub(super) fn resolve_parasolid_group_member_targets(
    ctx: &DecodeContext<'_>,
    members: &mut [ParasolidGroupMember],
    parsed: &ParsedStreams<'_>,
) -> Result<(), CodecError> {
    for index in ctx.admit_iter(&(0..members.len()), "NX GROUP member targets")? {
        let member = &mut members[index];
        let Ok(partition) = usize::try_from(member.partition_stream_ordinal) else {
            continue;
        };
        let graph = parsed.stream(partition).view_for_geometry().graph.as_ref();
        member.target = member.target.resolve(ctx, graph, member.member_xmt)?;
    }
    Ok(())
}

/// One completely bounded record in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ParasolidDeltasRecordWire")]
pub(super) struct ParasolidDeltasRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Semantic family, including POINT position and GROUP controls.
    pub(super) family: crate::deltas::record_family::RecordFamily,
    /// Stream-local XMT identity.
    pub(super) xmt: u32,
    /// Exact serialized record length.
    pub(super) byte_len: u64,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

impl Serialize for ParasolidDeltasRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use crate::deltas::record_family::RecordFamilyReferences;
        use serde::ser::SerializeStruct;

        let (group_selector, group_linked_reference_status) = match &self.family {
            RecordFamily::Group {
                selector,
                linked_reference_status,
                ..
            } => (Some(*selector), Some(*linked_reference_status)),
            _ => (None, None),
        };
        let mut wire = serializer.serialize_struct(
            "ParasolidDeltasRecordWire",
            10 + usize::from(group_selector.is_some())
                + usize::from(group_linked_reference_status.is_some()),
        )?;
        wire.serialize_field("id", &self.id)?;
        wire.serialize_field("stream_ordinal", &self.stream_ordinal)?;
        wire.serialize_field("family", self.family.family_name())?;
        wire.serialize_field("kind", &self.family.kind())?;
        wire.serialize_field("xmt", &self.xmt)?;
        wire.serialize_field("node_id", &self.family.node_id())?;
        wire.serialize_field("references", &RecordFamilyReferences(&self.family))?;
        if let Some(value) = group_selector {
            wire.serialize_field("group_selector", &value)?;
        }
        if let Some(value) = group_linked_reference_status {
            wire.serialize_field("group_linked_reference_status", &value)?;
        }
        wire.serialize_field("position", &self.family.position())?;
        wire.serialize_field("byte_len", &self.byte_len)?;
        wire.serialize_field("inflated_offset", &self.inflated_offset)?;
        wire.end()
    }
}

#[derive(Serialize, Deserialize)]
struct ParasolidDeltasRecordWire {
    id: String,
    stream_ordinal: u32,
    family: String,
    kind: u16,
    xmt: u32,
    node_id: Option<u32>,
    references: Vec<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_selector"
    )]
    group_selector: Option<GroupSelector>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_linked_reference_status"
    )]
    group_linked_reference_status: Option<GroupReferenceStatus>,
    position: Option<[f64; 3]>,
    byte_len: u64,
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidDeltasRecord> for ParasolidDeltasRecordWire {
    fn from(value: ParasolidDeltasRecord) -> Self {
        let (group_selector, group_linked_reference_status) = match &value.family {
            crate::deltas::record_family::RecordFamily::Group {
                selector,
                linked_reference_status,
                ..
            } => (Some(*selector), Some(*linked_reference_status)),
            _ => (None, None),
        };
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            family: value.family.family_name().to_string(),
            kind: value.family.kind(),
            xmt: value.xmt,
            node_id: value.family.node_id(),
            references: value.family.references(),
            group_selector,
            group_linked_reference_status,
            position: value.family.position(),
            byte_len: value.byte_len,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidDeltasRecordWire> for ParasolidDeltasRecord {
    type Error = String;

    fn try_from(wire: ParasolidDeltasRecordWire) -> Result<Self, Self::Error> {
        if wire.family == "POINT" {
            if let Some(position) = wire.position {
                crate::deltas::record_family::PointCoordinates::try_from(position)?;
            }
        }
        let family = crate::deltas::record_family::RecordFamily::from_wire(
            &wire.family,
            wire.kind,
            wire.node_id,
            wire.position,
            wire.group_selector,
            wire.group_linked_reference_status,
            wire.references,
        )
        .ok_or_else(|| {
            "deltas record family disagrees with kind, references, or payload".to_owned()
        })?;
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            family,
            xmt: wire.xmt,
            byte_len: wire.byte_len,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod deltas_record_wire_tests {
    use super::{ParasolidDeltasRecord, ParasolidDeltasRecordWire};

    #[test]
    fn deltas_record_borrowed_wire_matches_owned_bytes() {
        for json in [
            r#"{"id":"nx:deltas:record#group","stream_ordinal":0,"family":"GROUP","kind":90,"xmt":10,"node_id":7,"references":[3,4,5,6,30],"group_selector":4,"group_linked_reference_status":0,"position":null,"byte_len":22,"inflated_offset":0}"#,
            r#"{"id":"nx:deltas:record#type70","stream_ordinal":0,"family":"TYPE_70","kind":70,"xmt":6,"node_id":0,"references":[3,1,1,0,52,52],"position":null,"byte_len":32,"inflated_offset":0}"#,
            r#"{"id":"nx:deltas:record#empty","stream_ordinal":0,"family":"ENTITY_52","kind":82,"xmt":40,"node_id":null,"references":[],"position":null,"byte_len":10,"inflated_offset":0}"#,
        ] {
            let record: ParasolidDeltasRecord = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&ParasolidDeltasRecordWire::from(record.clone())).unwrap()
            );
        }
    }

    #[test]
    fn deltas_record_retained_limit_refuses_before_reference_collection() {
        let json = r#"{"id":"nx:deltas:record#group","stream_ordinal":0,"family":"GROUP","kind":90,"xmt":10,"node_id":7,"references":[3,4,5,6,30],"group_selector":4,"group_linked_reference_status":0,"position":null,"byte_len":22,"inflated_offset":0}"#;
        let record: ParasolidDeltasRecord = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

    #[test]
    fn record_family_owns_fixed_and_empty_reference_payloads() {
        for json in [
            r#"{"id":"group","stream_ordinal":0,"family":"GROUP","kind":90,"xmt":10,"node_id":7,"references":[3,4,5,6,30],"group_selector":4,"group_linked_reference_status":0,"position":null,"byte_len":22,"inflated_offset":0}"#,
            r#"{"id":"list","stream_ordinal":0,"family":"TYPE_91","kind":91,"xmt":30,"node_id":null,"references":[10,100,3,4,20,1],"position":null,"byte_len":26,"inflated_offset":0}"#,
            r#"{"id":"value","stream_ordinal":0,"family":"ENTITY_52","kind":82,"xmt":40,"node_id":null,"references":[],"position":null,"byte_len":10,"inflated_offset":0}"#,
        ] {
            let record: ParasolidDeltasRecord = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            let references = wire["references"].as_array_mut().unwrap();
            if references.is_empty() {
                references.push(serde_json::json!(1));
            } else {
                references.pop();
            }
            assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire)
                .unwrap_err()
                .to_string()
                .contains("references"));
        }
    }

    #[test]
    fn type_70_generates_the_repeated_trailing_reference() {
        let json = r#"{"id":"type70","stream_ordinal":0,"family":"TYPE_70","kind":70,"xmt":6,"node_id":0,"references":[3,1,1,0,52,52],"position":null,"byte_len":32,"inflated_offset":0}"#;
        let record: ParasolidDeltasRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        for references in [[3, 1, 1, 0, 52, 53], [3, 1, 1, 0, 0, 0], [3, 1, 1, 0, 1, 1]] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire["references"] = serde_json::json!(references);
            assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire)
                .unwrap_err()
                .to_string()
                .contains("references"));
        }
    }

    #[test]
    fn entity_51_retains_the_bounded_trailing_lane() {
        for count in [6, 37] {
            let references = vec![0; count];
            let json = format!(
                r#"{{"id":"entity","stream_ordinal":0,"family":"ENTITY_51","kind":81,"xmt":10,"node_id":null,"references":{},"position":null,"byte_len":32,"inflated_offset":0}}"#,
                serde_json::to_string(&references).unwrap()
            );
            let record: ParasolidDeltasRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
            for invalid_count in [0, 5, 38] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire["references"] = serde_json::json!(vec![0; invalid_count]);
                assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire)
                    .unwrap_err()
                    .to_string()
                    .contains("references"));
            }
        }
    }

    #[test]
    fn curve_descriptor_retains_both_reference_layouts() {
        for references in [vec![0, 1], vec![3, 4, 5]] {
            let json = format!(
                r#"{{"id":"curve","stream_ordinal":0,"family":"B_CURVE_DESCRIPTOR","kind":136,"xmt":20,"node_id":null,"references":{},"position":null,"byte_len":32,"inflated_offset":0}}"#,
                serde_json::to_string(&references).unwrap()
            );
            let record: ParasolidDeltasRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
            for invalid in [
                vec![],
                vec![3],
                vec![3, 4, 5, 6],
                vec![0, 4, 5],
                vec![3, 1, 5],
                vec![3, 4, 0],
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire["references"] = serde_json::json!(invalid);
                assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire)
                    .unwrap_err()
                    .to_string()
                    .contains("references"));
            }
        }
    }

    #[test]
    fn attdef_list_references_preserve_the_active_and_null_slots() {
        for references in [vec![1, 1], vec![1, 20], vec![1, 20, 21, 1]] {
            let json = format!(
                r#"{{"id":"slots","stream_ordinal":0,"family":"ATTDEF_LIST","kind":74,"xmt":20,"node_id":null,"references":{},"position":null,"byte_len":32,"inflated_offset":0}}"#,
                serde_json::to_string(&references).unwrap()
            );
            let record: ParasolidDeltasRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
            for invalid in [vec![], vec![1], vec![0, 20], vec![1, 0], vec![1, 20, 1, 21]] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire["references"] = serde_json::json!(invalid);
                assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire)
                    .unwrap_err()
                    .to_string()
                    .contains("references"));
            }
        }
    }

    #[test]
    fn point_wire_preserves_signed_zero_and_rejects_subnormal_coordinates() {
        let json = r#"{"id":"point","stream_ordinal":0,"family":"POINT","kind":29,"xmt":20,"node_id":7,"references":[1,1,1,1],"position":[0.0,-0.0,1.5],"byte_len":32,"inflated_offset":0}"#;
        let record: ParasolidDeltasRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        assert!(
            serde_json::from_str::<ParasolidDeltasRecord>(&json.replace("1.5", "5e-324"))
                .unwrap_err()
                .to_string()
                .contains("position")
        );
    }
}

/// One compact deletion in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ParasolidDeltasTombstoneWire")]
pub(super) struct ParasolidDeltasTombstone {
    /// Globally unique event identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Numeric Parasolid node type.
    kind: crate::deltas::record_kind::RecordKind,
    /// Stream-local deleted XMT identity.
    xmt: u32,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[derive(Serialize)]
struct ParasolidDeltasTombstoneRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    family: &'static str,
    kind: u16,
    xmt: u32,
    byte_len: u64,
    inflated_offset: u64,
}

impl Serialize for ParasolidDeltasTombstone {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidDeltasTombstoneRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            family: self.kind.name(),
            kind: u16::from(self.kind.code()),
            xmt: self.xmt,
            byte_len: 6,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ParasolidDeltasTombstoneWire {
    /// Globally unique event identity.
    id: String,
    /// Zero-based source stream ordinal.
    stream_ordinal: u32,
    /// Stable Parasolid record-family name.
    family: String,
    /// Numeric Parasolid node type.
    kind: u16,
    /// Stream-local deleted XMT identity.
    xmt: u32,
    /// Exact compact tombstone length.
    byte_len: u64,
    /// Record tag offset in the inflated stream.
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidDeltasTombstone> for ParasolidDeltasTombstoneWire {
    fn from(value: ParasolidDeltasTombstone) -> Self {
        Self {
            family: value.kind.name().into(),
            kind: u16::from(value.kind.code()),
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            byte_len: 6,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidDeltasTombstoneWire> for ParasolidDeltasTombstone {
    type Error = &'static str;
    fn try_from(wire: ParasolidDeltasTombstoneWire) -> Result<Self, Self::Error> {
        if wire.byte_len != 6 {
            return Err("byte_len: compact tombstone must contain six bytes");
        }
        let kind = crate::deltas::record_kind::RecordKind::try_from(wire.kind)?;
        if kind.name() != wire.family {
            return Err("deltas tombstone family disagrees with its node kind");
        }
        Ok(Self {
            kind,
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tombstone_wire_tests {
    use super::{ParasolidDeltasTombstone, ParasolidDeltasTombstoneWire};

    #[test]
    fn tombstone_borrowed_wire_matches_owned_bytes() {
        let json = r#"{"id":"nx:parasolid:tombstone#0","stream_ordinal":0,"family":"BODY","kind":12,"xmt":3,"byte_len":6,"inflated_offset":10}"#;
        let record: ParasolidDeltasTombstone = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&ParasolidDeltasTombstoneWire::from(record.clone())).unwrap()
        );
    }

    #[test]
    fn tombstone_native_limit_refuses_before_family_copy() {
        let json = r#"{"id":"nx:parasolid:tombstone#0","stream_ordinal":0,"family":"BODY","kind":12,"xmt":3,"byte_len":6,"inflated_offset":10}"#;
        let record: ParasolidDeltasTombstone = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}

/// BODY revision envelope in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "body_revision_wire::RevisionWire")]
pub(super) struct ParasolidDeltasBodyRevision {
    /// Globally unique revision identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local BODY XMT identity.
    xmt: NonNullXmt,
    /// Monotonic kernel revision identity.
    node_id: u32,
    /// Eight ordered BODY references.
    references: [u32; 8],
    /// Prefix and state-tail lengths with a representable total.
    lengths: RevisionLengths,
    /// SHA-256 of the exact bounded state-tail bytes.
    state_tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// BODY tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Parasolid transmit header at the start of a deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "TransmitHeaderWire")]
pub(super) struct ParasolidDeltasTransmitHeader {
    /// Globally unique header identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    state: TransmitState,
    /// Exact header byte length.
    byte_len: u64,
    /// SHA-256 of the exact header bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
}

/// Null references at the boundary of a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "tail_wire::NullTailWire")]
pub(super) struct ParasolidDeltasTerminalNullReferences {
    /// Globally unique trailer identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Complete two- or four-reference trailer form.
    form: NullTailForm,
    /// First trailer byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Count-selected numeric lane following one deltas `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "tail_wire::NumericTailWire")]
pub(super) struct ParasolidDeltasTermUseNumericTail {
    /// Globally unique numeric-tail identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// XMT identity of the owning `term_use` record.
    term_use_xmt: u32,
    /// Complete finite numeric tail for its endpoint count.
    values: NumericTailValues,
    /// First numeric byte following the complete `term_use` record.
    pub(super) inflated_offset: u64,
}

/// Maximal deltas gap composed entirely of typed stream-local references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasTaggedReferenceLane {
    /// Globally unique reference-lane identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Ordered `(Parasolid record kind, XMT identity)` references.
    references: TaggedReferences,
    /// Exact reference-lane byte length.
    byte_len: u64,
    /// SHA-256 of the exact reference-lane bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First byte of the first tagged reference.
    pub(super) inflated_offset: u64,
}

/// Framed reference/type map in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasReferenceTypeMap {
    /// Globally unique map identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Ordered `(XMT identity, Parasolid type code)` entries.
    entries: MapEntries,
    /// Type code of the optional terminal map target.
    #[serde(deserialize_with = "deserialize_map_target_kind")]
    target_kind: Option<std::num::NonZeroU16>,
    /// Exact map byte length.
    byte_len: u64,
    /// SHA-256 of the exact map bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First map byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

fn deserialize_map_target_kind<'de, D>(
    deserializer: D,
) -> Result<Option<std::num::NonZeroU16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    cadmpeg_core::absent_key::nullable::<D, u16>(deserializer)?
        .map(|kind| {
            std::num::NonZeroU16::new(kind).ok_or_else(|| {
                serde::de::Error::custom("target_kind: must be nonzero when present")
            })
        })
        .transpose()
}

/// Reference-state packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasReferenceStatePacket {
    /// Globally unique packet identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Ordered packet frames.
    frames: StateFrames,
    /// Whether the packet ends with `ref(1)[3], u32(1)`.
    terminal: bool,
    /// Exact packet byte length.
    byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First packet byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Schema reference preamble in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasSchemaReferencePreamble {
    /// Globally unique preamble identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    #[serde(flatten)]
    state: PreambleState,
    /// Exact preamble byte length.
    byte_len: u64,
    /// SHA-256 of the exact preamble bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First preamble byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Reference-marker packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasReferenceMarkerPacket {
    /// Globally unique packet identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Non-null stream-local XMT reference.
    reference: NonNullXmt,
    /// Serialized marker byte.
    marker: ReferenceMarker,
    /// Exact packet byte length.
    byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First packet byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Single-byte type-150 state packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasType150StatePacket {
    /// Globally unique packet identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Validated references, marker, and finite state values.
    #[serde(flatten)]
    state: Type150State,
    /// Exact packet byte length.
    byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First packet byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Inline schema declaration in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasInlineSchemaDeclaration {
    /// Globally unique declaration identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Schema-specific declaration body.
    #[serde(flatten)]
    fields: InlineSchemaFields,
    /// Exact declaration byte length.
    byte_len: u64,
    /// SHA-256 of the exact declaration bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First declaration byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Schema-bound type-12 `BODY` instance state in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasInlineBodyState {
    /// Globally unique state identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Serialized state form.
    fields: InlineBodyStateFields,
    /// Exact state byte length.
    byte_len: u64,
    /// SHA-256 of the exact state bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First state byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Maximal inflated-stream span outside every admitted deltas event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidDeltasResidualSpan {
    /// Globally unique residual-span identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Exact residual byte length.
    byte_len: u64,
    /// SHA-256 of the residual bytes.
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    /// First residual byte offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

pub(in crate::native) struct ParasolidDeltasEvents {
    pub(super) transmit_headers: Vec<ParasolidDeltasTransmitHeader>,
    pub(super) terminal_null_references: Vec<ParasolidDeltasTerminalNullReferences>,
    pub(super) records: Vec<ParasolidDeltasRecord>,
    pub(super) tombstones: Vec<ParasolidDeltasTombstone>,
    pub(super) body_revisions: Vec<ParasolidDeltasBodyRevision>,
    pub(super) term_use_numeric_tails: Vec<ParasolidDeltasTermUseNumericTail>,
    pub(super) tagged_reference_lanes: Vec<ParasolidDeltasTaggedReferenceLane>,
    pub(super) reference_type_maps: Vec<ParasolidDeltasReferenceTypeMap>,
    pub(super) reference_state_packets: Vec<ParasolidDeltasReferenceStatePacket>,
    pub(super) schema_reference_preambles: Vec<ParasolidDeltasSchemaReferencePreamble>,
    pub(super) reference_marker_packets: Vec<ParasolidDeltasReferenceMarkerPacket>,
    pub(super) type_150_state_packets: Vec<ParasolidDeltasType150StatePacket>,
    pub(super) inline_schema_declarations: Vec<ParasolidDeltasInlineSchemaDeclaration>,
    pub(super) inline_body_states: Vec<ParasolidDeltasInlineBodyState>,
    pub(super) residual_spans: Vec<ParasolidDeltasResidualSpan>,
}

/// Retain every completely bounded event in every Parasolid deltas stream.
#[cfg(test)]
fn parasolid_deltas_events(streams: &[Stream]) -> ParasolidDeltasEvents {
    crate::test_support::with_decode_context(|ctx| {
        let delta_censuses = streams
            .iter()
            .map(|stream| {
                if stream.kind() == crate::parasolid::StreamKind::Deltas {
                    Ok(Some(crate::deltas::census::walk(ctx, &stream.inflated)?))
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        parasolid_deltas_events_with_censuses(ctx, streams, delta_censuses)
    })
    .expect("bounded test deltas")
}

/// Format one retained deltas event identity after charging its exact length.
fn deltas_event_id(
    ctx: &DecodeContext<'_>,
    stream_ordinal: usize,
    kind: &'static str,
    first: usize,
    second: Option<u32>,
) -> Result<String, CodecError> {
    let digits = |value: u64| {
        value
            .checked_ilog10()
            .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
    };
    let length = "nx:s"
        .len()
        .checked_add(digits(cadmpeg_core::decode::u64_from_index(stream_ordinal)))
        .and_then(|length| length.checked_add(1 + kind.len() + 1))
        .and_then(|length| length.checked_add(digits(cadmpeg_core::decode::u64_from_index(first))))
        .and_then(|length| {
            second.map_or(Some(length), |value| {
                length.checked_add(1 + digits(u64::from(value)))
            })
        })
        .ok_or_else(|| ctx.refuse_codec_limit("NX deltas event identity", 0, 1))?;
    let mut id = ctx.retained_string(length, "NX deltas event identity")?;
    write!(&mut id, "nx:s{stream_ordinal}:{kind}#{first}")
        .map_err(|_| ctx.refuse_codec_limit("write NX deltas event identity", 0, 1))?;
    if let Some(value) = second {
        write!(&mut id, "-{value}")
            .map_err(|_| ctx.refuse_codec_limit("write NX deltas event identity", 0, 1))?;
    }
    Ok(id)
}

fn sort_deltas_events<T>(
    ctx: &DecodeContext<'_>,
    events: &mut [T],
    id: impl Fn(&T) -> &str,
) -> Result<(), CodecError> {
    ctx.stable_sort_by(events, |value| id(value), Ord::cmp, "sort NX deltas events")?;
    Ok(())
}

/// Retain deltas events from censuses produced by the shared decode substrate.
///
/// The function consumes the census vector after semantic construction has
/// finished, so the large record walk is performed once and its owned records
/// are moved directly into native output.
pub(super) fn parasolid_deltas_events_with_censuses(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
    mut delta_censuses: Vec<Option<Census>>,
) -> Result<ParasolidDeltasEvents, CodecError> {
    let mut events = ParasolidDeltasEvents {
        transmit_headers: Vec::new(),
        terminal_null_references: Vec::new(),
        records: Vec::new(),
        tombstones: Vec::new(),
        body_revisions: Vec::new(),
        term_use_numeric_tails: Vec::new(),
        tagged_reference_lanes: Vec::new(),
        reference_type_maps: Vec::new(),
        reference_state_packets: Vec::new(),
        schema_reference_preambles: Vec::new(),
        reference_marker_packets: Vec::new(),
        type_150_state_packets: Vec::new(),
        inline_schema_declarations: Vec::new(),
        inline_body_states: Vec::new(),
        residual_spans: Vec::new(),
    };
    for (stream_ordinal, stream) in ctx.admit_iter(streams, "NX deltas streams")?.enumerate() {
        if stream.kind() != crate::parasolid::StreamKind::Deltas {
            continue;
        }
        let stream_ordinal_u32 = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX deltas stream ordinal", 0, 1))?;
        let census = match delta_censuses
            .get_mut(stream_ordinal)
            .and_then(Option::take)
        {
            Some(census) => census,
            None => crate::deltas::census::walk(ctx, &stream.inflated)?,
        };
        let mut residual_start = 0;
        let (covered_spans, _covered_spans_storage) = census.covered_spans(ctx)?;
        for &(covered_start, covered_end) in
            ctx.admit_iter(&covered_spans, "NX deltas residual spans")?
        {
            if residual_start < covered_start {
                push_deltas_residual_span(
                    ctx,
                    &mut events.residual_spans,
                    stream_ordinal,
                    &stream.inflated,
                    residual_start,
                    covered_start,
                )?;
            }
            residual_start = residual_start.max(covered_end);
        }
        if residual_start < stream.inflated.len() {
            push_deltas_residual_span(
                ctx,
                &mut events.residual_spans,
                stream_ordinal,
                &stream.inflated,
                residual_start,
                stream.inflated.len(),
            )?;
        }
        let census = census.into_events();
        if let Some(header) = census.transmit_header {
            let bytes = &stream.inflated[..header.end];
            ctx.push_vec(
                &mut events.transmit_headers,
                ParasolidDeltasTransmitHeader {
                    id: deltas_event_id(ctx, stream_ordinal, "deltas-transmit-header", 0, None)?,
                    stream_ordinal: stream_ordinal_u32,
                    state: header.state,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                },
                "NX deltas events",
            )?;
        }
        if let Some(trailer) = census.terminal_null_references {
            ctx.push_vec(
                &mut events.terminal_null_references,
                ParasolidDeltasTerminalNullReferences {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-terminal-null-references",
                        trailer.offset(),
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    form: trailer.form(),
                    inflated_offset: cadmpeg_core::decode::u64_from_index(trailer.offset()),
                },
                "NX deltas events",
            )?;
        }
        for record in ctx.admit_iter(census.records, "NX deltas events")? {
            ctx.push_vec(
                &mut events.records,
                ParasolidDeltasRecord {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-record",
                        record.offset,
                        Some(record.xmt),
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    family: record.family,
                    xmt: record.xmt,
                    byte_len: cadmpeg_core::decode::u64_from_index(record.end - record.offset),
                    inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
                },
                "NX deltas events",
            )?;
        }
        for tombstone in ctx.admit_iter(census.tombstones, "NX deltas events")? {
            ctx.push_vec(
                &mut events.tombstones,
                ParasolidDeltasTombstone {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-tombstone",
                        tombstone.offset,
                        Some(tombstone.xmt),
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    kind: tombstone.kind,
                    xmt: tombstone.xmt,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(tombstone.offset),
                },
                "NX deltas events",
            )?;
        }
        for revision in ctx.admit_iter(census.body_revisions, "NX deltas events")? {
            let state_tail = &stream.inflated[revision.prefix_end..revision.end];
            ctx.push_vec(
                &mut events.body_revisions,
                ParasolidDeltasBodyRevision {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-body-revision",
                        revision.offset,
                        Some(revision.node_id),
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    xmt: revision.xmt,
                    node_id: revision.node_id,
                    references: revision.references,
                    lengths: RevisionLengths::from_slices(
                        &stream.inflated[revision.offset..revision.prefix_end],
                        state_tail,
                    ),
                    state_tail_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        state_tail,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(revision.offset),
                },
                "NX deltas events",
            )?;
        }
        for tail in ctx.admit_iter(census.term_use_numeric_tails, "NX deltas events")? {
            ctx.push_vec(
                &mut events.term_use_numeric_tails,
                ParasolidDeltasTermUseNumericTail {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-term-use-tail",
                        tail.offset(),
                        Some(tail.term_use_xmt),
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    term_use_xmt: tail.term_use_xmt,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(tail.offset()),
                    values: tail.into_values(),
                },
                "NX deltas events",
            )?;
        }
        for lane in ctx.admit_iter(census.tagged_reference_lanes, "NX deltas events")? {
            let bytes = &stream.inflated[lane.offset..lane.end];
            ctx.push_vec(
                &mut events.tagged_reference_lanes,
                ParasolidDeltasTaggedReferenceLane {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-tagged-reference-lane",
                        lane.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    references: lane.references,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(lane.offset),
                },
                "NX deltas events",
            )?;
        }
        for map in ctx.admit_iter(census.reference_type_maps, "NX deltas events")? {
            let bytes = &stream.inflated[map.offset..map.end];
            ctx.push_vec(
                &mut events.reference_type_maps,
                ParasolidDeltasReferenceTypeMap {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-reference-type-map",
                        map.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    entries: map.entries,
                    target_kind: map.target_kind,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(map.offset),
                },
                "NX deltas events",
            )?;
        }
        for packet in ctx.admit_iter(census.reference_state_packets, "NX deltas events")? {
            let bytes = &stream.inflated[packet.offset..packet.end];
            ctx.push_vec(
                &mut events.reference_state_packets,
                ParasolidDeltasReferenceStatePacket {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-reference-state",
                        packet.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    frames: packet.frames,
                    terminal: packet.terminal,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(packet.offset),
                },
                "NX deltas events",
            )?;
        }
        for preamble in ctx.admit_iter(census.schema_reference_preambles, "NX deltas events")? {
            let bytes = &stream.inflated[preamble.offset..preamble.end];
            ctx.push_vec(
                &mut events.schema_reference_preambles,
                ParasolidDeltasSchemaReferencePreamble {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-schema-reference-preamble",
                        preamble.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    state: preamble.state,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(preamble.offset),
                },
                "NX deltas events",
            )?;
        }
        for packet in ctx.admit_iter(census.reference_marker_packets, "NX deltas events")? {
            let bytes = &stream.inflated[packet.offset..packet.end];
            ctx.push_vec(
                &mut events.reference_marker_packets,
                ParasolidDeltasReferenceMarkerPacket {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-reference-marker",
                        packet.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    reference: packet.reference,
                    marker: packet.marker,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(packet.offset),
                },
                "NX deltas events",
            )?;
        }
        for packet in ctx.admit_iter(census.type_150_state_packets, "NX deltas events")? {
            let bytes = &stream.inflated[packet.offset..packet.end];
            ctx.push_vec(
                &mut events.type_150_state_packets,
                ParasolidDeltasType150StatePacket {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-type-150-state",
                        packet.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    state: packet.state,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(packet.offset),
                },
                "NX deltas events",
            )?;
        }
        for declaration in ctx.admit_iter(census.inline_schema_declarations, "NX deltas events")? {
            let bytes = &stream.inflated[declaration.offset..declaration.end];
            ctx.push_vec(
                &mut events.inline_schema_declarations,
                ParasolidDeltasInlineSchemaDeclaration {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-inline-schema",
                        declaration.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    fields: declaration.fields,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(declaration.offset),
                },
                "NX deltas events",
            )?;
        }
        for state in ctx.admit_iter(census.inline_body_states, "NX deltas events")? {
            let bytes = &stream.inflated[state.offset..state.end];
            ctx.push_vec(
                &mut events.inline_body_states,
                ParasolidDeltasInlineBodyState {
                    id: deltas_event_id(
                        ctx,
                        stream_ordinal,
                        "deltas-inline-body-state",
                        state.offset,
                        None,
                    )?,
                    stream_ordinal: stream_ordinal_u32,
                    fields: state.fields,
                    byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
                    sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                        ctx,
                        bytes,
                        "NX retained record digest",
                    )?,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(state.offset),
                },
                "NX deltas events",
            )?;
        }
    }
    sort_deltas_events(ctx, &mut events.transmit_headers, |value| value.id.as_str())?;
    sort_deltas_events(ctx, &mut events.terminal_null_references, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.records, |value| value.id.as_str())?;
    sort_deltas_events(ctx, &mut events.tombstones, |value| value.id.as_str())?;
    sort_deltas_events(ctx, &mut events.body_revisions, |value| value.id.as_str())?;
    sort_deltas_events(ctx, &mut events.term_use_numeric_tails, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.tagged_reference_lanes, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.reference_type_maps, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.reference_state_packets, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.schema_reference_preambles, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.reference_marker_packets, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.type_150_state_packets, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.inline_schema_declarations, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.inline_body_states, |value| {
        value.id.as_str()
    })?;
    sort_deltas_events(ctx, &mut events.residual_spans, |value| value.id.as_str())?;
    Ok(events)
}

fn push_deltas_residual_span(
    ctx: &DecodeContext<'_>,
    residual_spans: &mut Vec<ParasolidDeltasResidualSpan>,
    stream_ordinal: usize,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<(), CodecError> {
    let residual = &bytes[start..end];
    ctx.push_vec(
        residual_spans,
        ParasolidDeltasResidualSpan {
            id: deltas_event_id(ctx, stream_ordinal, "deltas-residual", start, None)?,
            stream_ordinal: u32::try_from(stream_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX deltas residual stream ordinal", 0, 1))?,
            byte_len: cadmpeg_core::decode::u64_from_index(residual.len()),
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                residual,
                "NX retained record digest",
            )?,
            inflated_offset: cadmpeg_core::decode::u64_from_index(start),
        },
        "NX deltas events",
    )
}

/// Shared skeleton for Parasolid record families read from the cached per-stream
/// record view. It owns the stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}`
/// identity, and the sort by identity; each family supplies only its cached row
/// slice and its record constructor.
trait ParasolidStreamRecords {
    /// Cached row type read from the stream's record [`StreamView`].
    type Row: Copy;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// The cached rows of one stream's record view.
    fn rows(view: &StreamView) -> &[Self::Row];
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and cached row.
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

fn parasolid_record_id(
    ctx: &DecodeContext<'_>,
    stream_ordinal: usize,
    stem: &'static str,
    xmt: u32,
) -> Result<String, CodecError> {
    let id_len = "nx:s"
        .len()
        .checked_add(
            stream_ordinal
                .checked_ilog10()
                .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
        )
        .and_then(|length| length.checked_add(1 + stem.len() + 1))
        .and_then(|length| {
            length.checked_add(
                xmt.checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
            )
        })
        .ok_or_else(|| ctx.refuse_codec_limit("retain NX Parasolid record id", 0, 1))?;
    let mut id = ctx.retained_string(id_len, "retain NX Parasolid record id")?;
    write!(&mut id, "nx:s{stream_ordinal}:{stem}#{xmt}")
        .map_err(|_| ctx.refuse_codec_limit("write NX Parasolid record id", 0, 1))?;
    Ok(id)
}

fn parasolid_offset_record_id(
    ctx: &DecodeContext<'_>,
    stream_ordinal: usize,
    stem: &'static str,
    xmt: u32,
    offset: usize,
) -> Result<String, CodecError> {
    let id_len = "nx:s"
        .len()
        .checked_add(
            stream_ordinal
                .checked_ilog10()
                .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
        )
        .and_then(|length| length.checked_add(1 + stem.len() + 1))
        .and_then(|length| {
            length.checked_add(
                xmt.checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
            )
        })
        .and_then(|length| length.checked_add(1))
        .and_then(|length| {
            length.checked_add(
                offset
                    .checked_ilog10()
                    .map_or(1, |digits| cadmpeg_core::decode::index_from_u32(digits) + 1),
            )
        })
        .ok_or_else(|| ctx.refuse_codec_limit("retain NX Parasolid offset record id", 0, 1))?;
    let mut id = ctx.retained_string(id_len, "retain NX Parasolid offset record id")?;
    write!(&mut id, "nx:s{stream_ordinal}:{stem}#{xmt}-{offset}")
        .map_err(|_| ctx.refuse_codec_limit("write NX Parasolid offset record id", 0, 1))?;
    Ok(id)
}

/// Run the cached-view record skeleton for one family: map every cached row of
/// every stream to a record, then sort by identity. Non-Parasolid streams hold
/// empty views, so no per-stream guard is needed.
fn per_parasolid_stream<P: ParasolidStreamRecords>(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams,
) -> Result<Vec<P::Record>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        ctx.charge_work(1, "NX Parasolid cached streams")?;
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX Parasolid stream ordinal", 0, 1))?;
        for row in ctx.admit_iter(
            P::rows(stream.view_for_records()),
            "NX Parasolid cached records",
        )? {
            ctx.reserve_vec(&mut records, 1, "NX Parasolid cached records")?;
            let id = parasolid_record_id(ctx, stream_ordinal, P::ID_STEM, P::xmt(row))?;
            records.push(P::record(id, ordinal, row));
        }
    }
    ctx.stable_sort_by(
        &mut records,
        |value| P::id(value),
        Ord::cmp,
        "sort NX Parasolid records",
    )?;
    Ok(records)
}

/// Shared skeleton for Parasolid record families scanned fresh from each
/// Parasolid stream's inflated bytes. It owns the `is_parasolid()` guard, the
/// stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}` identity, and the sort; each
/// family supplies only its scanner and its record constructor.
trait ParasolidScanRecords {
    /// Scanned row type produced from the inflated stream bytes.
    type Row;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// Scan one inflated Parasolid stream into its rows.
    fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Self::Row>, CodecError>;
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and scanned row.
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the fresh-scan record skeleton for one family: scan every Parasolid
/// stream, map each scanned row to a record, then sort by identity.
fn per_parasolid_scan<P: ParasolidScanRecords>(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<P::Record>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in ctx
        .admit_iter(streams, "NX Parasolid scanned streams")?
        .enumerate()
    {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX Parasolid scan ordinal", 0, 1))?;
        let rows = P::scan(ctx, &stream.inflated)?;
        for row in ctx.admit_iter(rows, "NX Parasolid scanned records")? {
            ctx.reserve_vec(&mut records, 1, "NX Parasolid scanned records")?;
            let id = parasolid_record_id(ctx, stream_ordinal, P::ID_STEM, P::xmt(&row))?;
            records.push(P::record(id, ordinal, row));
        }
    }
    ctx.stable_sort_by(
        &mut records,
        |value| P::id(value),
        Ord::cmp,
        "sort NX Parasolid records",
    )?;
    Ok(records)
}

/// Complete typed source record for one Parasolid offset surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidOffsetSurfaceRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the offset surface.
    xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    discriminator: crate::topology::OffsetSurfaceDiscriminator,
    /// Serialized true-offset flag.
    true_offset: bool,
    /// Checked support reference and signed model distance.
    #[serde(flatten)]
    state: OffsetSurfaceState,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid offset surfaces.
pub(super) fn parasolid_offset_surface_records(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams,
) -> Result<Vec<ParasolidOffsetSurfaceRecord>, CodecError> {
    per_parasolid_stream::<ParasolidOffsetSurfaceRecord>(ctx, parsed)
}

impl ParasolidStreamRecords for ParasolidOffsetSurfaceRecord {
    type Row = crate::topology::OffsetSurface;
    type Record = ParasolidOffsetSurfaceRecord;
    const ID_STEM: &'static str = "offset-surface-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.offset_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidOffsetSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            discriminator: row.discriminator,
            true_offset: row.true_offset,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid trimmed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidTrimmedCurveRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the trimmed curve.
    xmt: u32,
    #[serde(flatten)]
    state: crate::topology::trimmed_curve_state::TrimmedCurveState,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid trimmed curves.
pub(super) fn parasolid_trimmed_curve_records(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams,
) -> Result<Vec<ParasolidTrimmedCurveRecord>, CodecError> {
    per_parasolid_stream::<ParasolidTrimmedCurveRecord>(ctx, parsed)
}

impl ParasolidStreamRecords for ParasolidTrimmedCurveRecord {
    type Row = crate::topology::TrimmedCurve;
    type Record = ParasolidTrimmedCurveRecord;
    const ID_STEM: &'static str = "trimmed-curve-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.trimmed_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidTrimmedCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidSurfaceCurveRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the surface curve.
    xmt: u32,
    #[serde(flatten)]
    state: crate::topology::surface_curve_state::SurfaceCurveState,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid surface curves.
pub(super) fn parasolid_surface_curve_records(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams,
) -> Result<Vec<ParasolidSurfaceCurveRecord>, CodecError> {
    per_parasolid_stream::<ParasolidSurfaceCurveRecord>(ctx, parsed)
}

impl ParasolidStreamRecords for ParasolidSurfaceCurveRecord {
    type Row = crate::topology::SurfaceCurve;
    type Record = ParasolidSurfaceCurveRecord;
    const ID_STEM: &'static str = "surface-curve-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.surface_curves
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidSurfaceCurveRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid blend-bound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidBlendBoundRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    #[serde(flatten)]
    state: crate::intersection::blend_bound_state::BlendBoundState,
    /// Serialized partition/deltas and direct/escaped framing.
    framing: crate::intersection::BlendBoundFraming,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid blend-bound bridges.
pub(super) fn parasolid_blend_bound_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidBlendBoundRecord>, CodecError> {
    per_parasolid_scan::<ParasolidBlendBoundRecord>(ctx, streams)
}

impl ParasolidScanRecords for ParasolidBlendBoundRecord {
    type Row = crate::intersection::BlendBound;
    type Record = ParasolidBlendBoundRecord;
    const ID_STEM: &'static str = "blend-bound-record";
    fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Self::Row>, CodecError> {
        crate::intersection::blend_bounds(ctx, bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.state.xmt()
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidBlendBoundRecord {
            id,
            stream_ordinal,
            state: row.state,
            framing: row.framing,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ParasolidTermUseRecordWire")]
pub(super) struct ParasolidTermUseRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    xmt: NonNullXmt,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    form: crate::intersection::TermUseForm,
    /// Endpoint position in millimetres.
    point: FiniteVector<3>,
    /// Serialized record framing.
    framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[derive(Serialize)]
struct ParasolidTermUseRecordRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: NonNullXmt,
    count: u32,
    form: crate::intersection::TermUseForm,
    point: FiniteVector<3>,
    framing: crate::intersection::TermUseFraming,
    inflated_offset: u64,
}

impl Serialize for ParasolidTermUseRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidTermUseRecordRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt,
            count: self.form.count(),
            form: self.form,
            point: self.point,
            framing: self.framing,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ParasolidTermUseRecordWire {
    /// Globally unique record identity.
    id: String,
    /// Zero-based source stream ordinal.
    stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    xmt: NonNullXmt,
    /// Serialized leading count.
    count: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    form: crate::intersection::TermUseForm,
    /// Endpoint position in millimetres.
    point: FiniteVector<3>,
    /// Serialized record framing.
    framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidTermUseRecord> for ParasolidTermUseRecordWire {
    fn from(value: ParasolidTermUseRecord) -> Self {
        Self {
            count: value.form.count(),
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            form: value.form,
            point: value.point,
            framing: value.framing,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidTermUseRecordWire> for ParasolidTermUseRecord {
    type Error = &'static str;
    fn try_from(wire: ParasolidTermUseRecordWire) -> Result<Self, Self::Error> {
        if wire.count != wire.form.count() {
            return Err("term_use count disagrees with endpoint form");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            form: wire.form,
            point: wire.point,
            framing: wire.framing,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod term_use_wire_tests {
    use super::ParasolidTermUseRecord;

    #[test]
    fn finite_endpoint_retains_the_native_point_array_and_checked_identity() {
        let json = r#"{"id":"term","stream_ordinal":0,"xmt":2,"count":2,"form":"TF","point":[0.0,-0.0,1.0],"framing":"direct","inflated_offset":10}"#;
        let record: ParasolidTermUseRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&super::ParasolidTermUseRecordWire::from(record.clone())).unwrap()
        );
    }

    #[test]
    fn term_use_native_limit_refuses_before_id_copy() {
        let json = r#"{"id":"nx:parasolid:term-use#0","stream_ordinal":0,"xmt":2,"count":2,"form":"TF","point":[0.0,-0.0,1.0],"framing":"direct","inflated_offset":10}"#;
        let record: ParasolidTermUseRecord = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}

#[cfg(test)]
mod term_use_identity_tests {
    #[test]
    fn term_use_wire_rejects_reserved_record_identities() {
        for identity in [0, 1] {
            let json = format!(
                r#"{{"id":"term","stream_ordinal":0,"xmt":{identity},"count":2,"form":"TF","point":[0.0,0.0,1.0],"framing":"direct","inflated_offset":10}}"#
            );
            assert!(serde_json::from_str::<super::ParasolidTermUseRecord>(&json)
                .unwrap_err()
                .to_string()
                .contains("xmt"));
        }
    }
}

/// Decode complete typed source records for Parasolid `term_use` endpoints.
pub(super) fn parasolid_term_use_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidTermUseRecord>, CodecError> {
    per_parasolid_scan::<ParasolidTermUseRecord>(ctx, streams)
}

impl ParasolidScanRecords for ParasolidTermUseRecord {
    type Row = crate::intersection::TermUse;
    type Record = ParasolidTermUseRecord;
    const ID_STEM: &'static str = "term-use-record";
    fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Self::Row>, CodecError> {
        crate::intersection::term_use_records(ctx, bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        u32::from(row.xmt)
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidTermUseRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            form: row.form,
            point: row.point,
            framing: row.framing,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid support-UV values array.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "support_uv_wire::SupportUvWire")]
pub(super) struct ParasolidSupportUvRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the values array.
    xmt: NonNullXmt,
    /// Exact finite packed support tuples.
    values: crate::intersection::support_uv_values::SupportUvValues,
    /// Serialized record framing.
    framing: crate::intersection::SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid support-UV arrays.
pub(super) fn parasolid_support_uv_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidSupportUvRecord>, CodecError> {
    per_parasolid_scan::<ParasolidSupportUvRecord>(ctx, streams)
}

impl ParasolidScanRecords for ParasolidSupportUvRecord {
    type Row = crate::intersection::SupportUvRecord;
    type Record = ParasolidSupportUvRecord;
    const ID_STEM: &'static str = "support-uv-record";
    fn scan(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<Self::Row>, CodecError> {
        crate::intersection::support_uv_records(ctx, bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        u32::from(row.xmt)
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidSupportUvRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            values: row.values,
            framing: row.framing,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one physical Parasolid `CHART_s` record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "chart_wire::ChartWire")]
pub(super) struct ParasolidChartRecord {
    /// Globally unique physical-record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the chart.
    xmt: NonNullXmt,
    /// Checked chart preamble.
    preamble: crate::intersection::chart_samples::ChartPreamble,
    /// Points with exactly the fields admitted by their Hvec layout.
    data: crate::intersection::chart_samples::SourceChartData,
    /// Serialized record framing.
    framing: crate::intersection::ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode every complete physical Parasolid chart source record.
pub(super) fn parasolid_chart_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidChartRecord>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in ctx
        .admit_iter(streams, "NX Parasolid chart streams")?
        .enumerate()
    {
        let crate::parasolid::StreamBody::Parasolid { subtype, .. } = &stream.body else {
            continue;
        };
        let point_layout = subtype.chart_point_layout();
        let charts =
            crate::intersection::chart_source_records(ctx, &stream.inflated, point_layout)?;
        for chart in ctx.admit_iter(charts, "NX Parasolid chart records")? {
            ctx.reserve_vec(&mut records, 1, "NX Parasolid chart records")?;
            records.push(ParasolidChartRecord {
                id: parasolid_offset_record_id(
                    ctx,
                    stream_ordinal,
                    "chart-record",
                    u32::from(chart.xmt),
                    chart.pos,
                )?,
                stream_ordinal: u32::try_from(stream_ordinal).map_err(|_| {
                    ctx.refuse_codec_limit("NX Parasolid chart stream ordinal", 0, 1)
                })?,
                xmt: chart.xmt,
                preamble: chart.preamble,
                data: chart.data,
                framing: chart.framing,
                inflated_offset: cadmpeg_core::decode::u64_from_index(chart.pos),
            });
        }
    }
    ctx.stable_sort_by(
        &mut records,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid records",
    )?;
    Ok(records)
}

/// Complete typed source record for one Parasolid surface-intersection curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidIntersectionRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Cross-reference index of the construction.
    xmt: u32,
    /// Five ordered common-header references.
    header_references: [u32; 5],
    /// Serialized orientation sense.
    sense: bool,
    /// Six ordered support and witness references.
    construction_references: [u32; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub(super) delta_twin: bool,
    /// Record tag offset in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Decode complete typed source records for retained intersection constructions.
pub(super) fn parasolid_intersection_records(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams<'_>,
) -> Result<Vec<ParasolidIntersectionRecord>, CodecError> {
    per_parasolid_stream::<ParasolidIntersectionRecord>(ctx, parsed)
}

impl ParasolidStreamRecords for ParasolidIntersectionRecord {
    type Row = crate::topology::CompositeCurve;
    type Record = ParasolidIntersectionRecord;
    const ID_STEM: &'static str = "intersection-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.intersections.source_constructions
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidIntersectionRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            header_references: row
                .header_references
                .map(crate::framing::xmt_reference::XmtTarget::to_wire),
            sense: row.sense,
            construction_references: row
                .references
                .map(crate::framing::xmt_reference::XmtTarget::to_wire),
            delta_twin: row.delta_twin,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed type-56 rolling-ball blend-surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidBlendSurfaceRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based embedded Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local `BLEND_SURF` identity.
    xmt: u32,
    /// Checked supports, offsets, and thumb weights.
    #[serde(flatten)]
    state: BlendSurfaceState,
    /// Offset of the type tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
mod blend_surface_wire_tests {
    use super::ParasolidBlendSurfaceRecord;

    #[test]
    fn checked_blend_state_retains_the_flat_native_wire() {
        let json = r#"{"id":"blend","stream_ordinal":0,"xmt":20,"support_xmts":[6,7],"spine_xmt":0,"offsets":[-3.0,3.0],"thumb_weights":[-0.0,-2.0],"inflated_offset":10}"#;
        let record: ParasolidBlendSurfaceRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        for (field, value) in [
            ("support_xmts", serde_json::json!([1, 7])),
            ("offsets", serde_json::json!([3.0, 4.0])),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = value;
            assert!(serde_json::from_value::<ParasolidBlendSurfaceRecord>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}

fn default_legal_owner_flag_count() -> u8 {
    16
}

fn is_default_legal_owner_flag_count(value: impl std::borrow::Borrow<u8>) -> bool {
    *value.borrow() == default_legal_owner_flag_count()
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ParasolidAttributeDefinitionWire")]
pub(super) struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based embedded stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub(super) xmt: NonNullXmt,
    /// Optional stream-local next-definition target.
    pub(super) next_definition_xmt: Option<XmtTarget>,
    /// Stream-local type-79 identifier identity.
    pub(super) identifier_xmt: NonNullXmt,
    /// Offset of the resolved type-79 identifier in the inflated stream.
    pub(super) identifier_inflated_offset: u64,
    /// Exact printable attribute class name.
    pub(super) name: PrintableString<String>,
    /// Numeric attribute type identifier.
    pub(super) type_id: NonZeroU32,
    /// Ordered actions for the eight logged event families.
    pub(super) action_codes: [AttributeAction; 8],
    /// Optional stream-local field-name-list target.
    pub(super) field_names_xmt: Option<XmtTarget>,
    /// Ordered legal-owner flags.
    pub(super) legal_owner_flags: crate::parasolid::LegalOwnerFlags,
    /// One serialized code for every declared field.
    pub(super) field_codes: Vec<AttributeField>,
    /// Offset of the declaration in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[derive(Serialize)]
struct ParasolidAttributeDefinitionRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: u32,
    next_definition_xmt: u32,
    identifier_xmt: u32,
    identifier_inflated_offset: u64,
    name: &'a str,
    type_id: u32,
    action_codes: [AttributeAction; 8],
    field_names_xmt: u32,
    legal_owner_flags: [u8; 16],
    #[serde(skip_serializing_if = "is_default_legal_owner_flag_count")]
    legal_owner_flag_count: u8,
    field_count: usize,
    field_codes: &'a [AttributeField],
    inflated_offset: u64,
}

impl Serialize for ParasolidAttributeDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidAttributeDefinitionRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt.into(),
            next_definition_xmt: XmtTarget::to_wire(self.next_definition_xmt),
            identifier_xmt: self.identifier_xmt.into(),
            identifier_inflated_offset: self.identifier_inflated_offset,
            name: self.name.as_str(),
            type_id: self.type_id.get(),
            action_codes: self.action_codes,
            field_names_xmt: XmtTarget::to_wire(self.field_names_xmt),
            legal_owner_flags: self.legal_owner_flags.padded(),
            legal_owner_flag_count: u8::try_from(self.legal_owner_flags.as_slice().len())
                .map_err(serde::ser::Error::custom)?,
            field_count: self.field_codes.len(),
            field_codes: &self.field_codes,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ParasolidAttributeDefinitionWire {
    /// Globally unique native-record identity.
    id: String,
    /// Zero-based embedded stream ordinal.
    stream_ordinal: u32,
    /// Stream-local definition record identity.
    xmt: u32,
    /// Stream-local next-definition identity; `1` is null.
    next_definition_xmt: u32,
    /// Stream-local type-79 identifier identity.
    identifier_xmt: u32,
    /// Offset of the resolved type-79 identifier in the inflated stream.
    identifier_inflated_offset: u64,
    /// Exact printable attribute class name.
    name: String,
    /// Numeric attribute type identifier.
    type_id: u32,
    /// Ordered actions for the eight logged event families.
    action_codes: [AttributeAction; 8],
    /// Stream-local field-name-list identity; `1` is null.
    field_names_xmt: u32,
    /// Ordered legal-owner flags.
    legal_owner_flags: [u8; 16],
    /// Number of legal-owner flags serialized by the definition.
    #[serde(
        default = "default_legal_owner_flag_count",
        skip_serializing_if = "is_default_legal_owner_flag_count"
    )]
    legal_owner_flag_count: u8,
    /// Declared number of fields.
    field_count: usize,
    /// One serialized code for every declared field.
    field_codes: Vec<AttributeField>,
    /// Offset of the declaration in the inflated stream.
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidAttributeDefinition> for ParasolidAttributeDefinitionWire {
    fn from(value: ParasolidAttributeDefinition) -> Self {
        Self {
            legal_owner_flag_count: u8::try_from(value.legal_owner_flags.as_slice().len())
                .expect("fixture value fits u8"),
            legal_owner_flags: value.legal_owner_flags.padded(),
            field_count: value.field_codes.len(),
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt.into(),
            next_definition_xmt: XmtTarget::to_wire(value.next_definition_xmt),
            identifier_xmt: value.identifier_xmt.into(),
            identifier_inflated_offset: value.identifier_inflated_offset,
            name: value.name.into_inner(),
            type_id: value.type_id.get(),
            action_codes: value.action_codes,
            field_names_xmt: XmtTarget::to_wire(value.field_names_xmt),
            field_codes: value.field_codes,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidAttributeDefinitionWire> for ParasolidAttributeDefinition {
    type Error = &'static str;
    fn try_from(wire: ParasolidAttributeDefinitionWire) -> Result<Self, Self::Error> {
        let count = usize::from(wire.legal_owner_flag_count);
        let flags = wire
            .legal_owner_flags
            .get(..count)
            .ok_or("attribute owner flag count exceeds the wire array")?;
        let legal_owner_flags = crate::parasolid::LegalOwnerFlags::try_from(flags)?;
        if wire.legal_owner_flags != legal_owner_flags.padded() {
            return Err("attribute owner flag padding is nonzero");
        }
        if wire.field_count != wire.field_codes.len() {
            return Err("attribute field count disagrees with field codes");
        }
        Ok(Self {
            legal_owner_flags,
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: NonNullXmt::try_from(wire.xmt).map_err(|_| "xmt must exceed one")?,
            next_definition_xmt: XmtTarget::from_wire(wire.next_definition_xmt),
            identifier_xmt: NonNullXmt::try_from(wire.identifier_xmt)
                .map_err(|_| "identifier_xmt must exceed one")?,
            identifier_inflated_offset: wire.identifier_inflated_offset,
            name: PrintableString::new(wire.name)
                .map_err(|_| "name must be nonempty printable ASCII")?,
            type_id: NonZeroU32::new(wire.type_id).ok_or("type_id must be nonzero")?,
            action_codes: wire.action_codes,
            field_names_xmt: XmtTarget::from_wire(wire.field_names_xmt),
            field_codes: wire.field_codes,
            inflated_offset: wire.inflated_offset,
        })
    }
}

/// Counted Parasolid type-99 field-name reference record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidFieldNamesRecord {
    /// Globally unique native-record identity.
    id: String,
    /// Zero-based embedded stream ordinal.
    stream_ordinal: u32,
    /// Stream-local record identity.
    xmt: NonNullXmt,
    /// Ordered stream-local character or Unicode value references.
    name_xmts: NameReferences,
    /// Exact framed record length.
    byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    inflated_offset: u64,
}

/// Complete type-80 declaration-to-field-name-list relation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "named_fields::FieldNamesWire")]
pub(super) struct ParasolidAttributeFieldNames {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Zero-based embedded stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-80 declaration.
    pub(super) attribute_definition: String,
    /// Uniquely resolved type-99 field-name record.
    pub(super) field_names_record: String,
    /// Ordered exact names paired with their resolved value records.
    pub(super) fields: Vec<NamedField>,
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Parasolid topology record type.
    pub(super) topology_type: TopologyAttributeKind,
    /// Stream-local topology-record identity.
    pub(super) topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub(super) attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_attribute_list_record"
    )]
    pub(super) attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Entity51Wire")]
pub(super) struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    xmt: NonNullXmt,
    /// Serialized sequence value.
    sequence: NonZeroU32,
    /// Stream-local type-80 attribute-definition identity.
    definition_xmt: u32,
    /// Five fixed leading stream-local references.
    leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    trailing_references: EntityReferences,
    /// Exact framed record length.
    byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    xmt: NonNullXmt,
    /// Exact nonempty printable value.
    pub(super) value: PrintableString<String>,
    /// Exact framed record length.
    byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered big-endian unsigned values.
    pub(super) values: CountedValues<u32>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered finite big-endian binary64 values.
    pub(super) values: CountedValues<f64>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Parasolid vector-shaped attribute-value family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ParasolidVectorValueKind {
    /// Type-85 point values.
    Points,
    /// Type-86 free-vector values.
    Vectors,
    /// Type-89 direction values.
    Directions,
}

/// Counted Parasolid type-85, type-86, or type-89 vector-shaped value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidEntityVectorRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Exact value family.
    pub(super) kind: ParasolidVectorValueKind,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered finite xyz values.
    pub(super) values: CountedValues<[f64; 3]>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-87 axis-value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity57AxisRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered axes, each retaining its two serialized xyz vectors.
    pub(super) values: CountedValues<[[f64; 3]; 2]>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-88 tag-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity58TagRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Ordered exact tag values.
    pub(super) values: CountedValues<u32>,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Counted Parasolid type-98 Unicode-value record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ParasolidEntity62UnicodeRecordWire")]
pub(super) struct ParasolidEntity62UnicodeRecord {
    /// Globally unique native-record identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Stream-local record identity.
    pub(super) xmt: NonNullXmt,
    /// Validated Unicode scalar string.
    pub(super) value: UnicodeValue,
    /// Exact framed record length.
    pub(super) byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub(super) inflated_offset: u64,
}

struct UnicodeCodeUnits<'a>(&'a str);

impl Serialize for UnicodeCodeUnits<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut units = serializer.serialize_seq(Some(self.0.encode_utf16().count()))?;
        for unit in self.0.encode_utf16() {
            units.serialize_element(&unit)?;
        }
        units.end()
    }
}

#[derive(Serialize)]
struct ParasolidEntity62UnicodeRecordRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: NonNullXmt,
    code_units: UnicodeCodeUnits<'a>,
    value: &'a UnicodeValue,
    byte_len: u64,
    inflated_offset: u64,
}

impl Serialize for ParasolidEntity62UnicodeRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidEntity62UnicodeRecordRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt,
            code_units: UnicodeCodeUnits(self.value.as_str()),
            value: &self.value,
            byte_len: self.byte_len,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct ParasolidEntity62UnicodeRecordWire {
    id: String,
    stream_ordinal: u32,
    xmt: NonNullXmt,
    code_units: Vec<u16>,
    value: UnicodeValue,
    byte_len: u64,
    inflated_offset: u64,
}

#[cfg(test)]
impl From<ParasolidEntity62UnicodeRecord> for ParasolidEntity62UnicodeRecordWire {
    fn from(value: ParasolidEntity62UnicodeRecord) -> Self {
        let code_units = value.value.as_str().encode_utf16().collect();
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            code_units,
            value: value.value,
            byte_len: value.byte_len,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<ParasolidEntity62UnicodeRecordWire> for ParasolidEntity62UnicodeRecord {
    type Error = &'static str;
    fn try_from(wire: ParasolidEntity62UnicodeRecordWire) -> Result<Self, Self::Error> {
        if !wire
            .value
            .as_str()
            .encode_utf16()
            .eq(wire.code_units.iter().copied())
        {
            return Err("ParasolidEntity62UnicodeRecord.code_units disagrees with value");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            value: wire.value,
            byte_len: wire.byte_len,
            inflated_offset: wire.inflated_offset,
        })
    }
}

/// Attribute-value records discovered by one pass over the Parasolid streams.
pub(in crate::native) struct ParasolidEntityValueRecords {
    pub(super) integers: Vec<ParasolidEntity52IntegerRecord>,
    pub(super) doubles: Vec<ParasolidEntity53DoubleRecord>,
    pub(super) strings: Vec<ParasolidEntity54StringRecord>,
    pub(super) vectors: Vec<ParasolidEntityVectorRecord>,
    pub(super) axes: Vec<ParasolidEntity57AxisRecord>,
    pub(super) tags: Vec<ParasolidEntity58TagRecord>,
    pub(super) unicode: Vec<ParasolidEntity62UnicodeRecord>,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-81 record.
    pub(super) entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub(super) position: FieldPosition,
    /// Stream-local referenced xmt.
    pub(super) referenced_xmt: NonNullXmt,
    /// Numeric record family.
    pub(super) kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub(super) value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-81 record.
    pub(super) entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub(super) position: FieldPosition,
    /// Stream-local referenced xmt.
    referenced_xmt: NonNullXmt,
    /// Uniquely resolved type-84 string record.
    pub(super) string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Exact type-81 reference to one uniquely resolved structured value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ParasolidEntity51StructuredUse {
    /// Globally unique use identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Owning type-81 record.
    pub(super) entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub(super) position: FieldPosition,
    /// Stream-local referenced xmt.
    pub(super) referenced_xmt: NonNullXmt,
    /// Structured value-record family.
    pub(super) kind: StructuredValueKind,
    /// Uniquely resolved structured value record.
    pub(super) value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

/// Resolved registered class of one Parasolid type-81 attribute instance.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(test), derive(Clone))]
pub(super) struct ParasolidAttributeClassUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Type-81 attribute-instance record.
    entity_51_record: String,
    /// Stream-local XMT of the matched type-80 definition.
    definition_xmt: NonNullXmt,
    /// Uniquely matched attribute definition.
    attribute_definition: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static ATTRIBUTE_CLASS_USE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidAttributeClassUse {
    fn clone(&self) -> Self {
        ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            stream_ordinal: self.stream_ordinal,
            entity_51_record: self.entity_51_record.clone(),
            definition_xmt: self.definition_xmt,
            attribute_definition: self.attribute_definition.clone(),
            inflated_offset: self.inflated_offset,
        }
    }
}

#[derive(Serialize)]
struct ParasolidAttributeClassUseRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    entity_51_record: &'a str,
    definition_xmt: NonNullXmt,
    attribute_definition: &'a str,
}

impl Serialize for ParasolidAttributeClassUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidAttributeClassUseRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            entity_51_record: &self.entity_51_record,
            definition_xmt: self.definition_xmt,
            attribute_definition: &self.attribute_definition,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct ParasolidAttributeClassUseWire {
    id: String,
    stream_ordinal: u32,
    entity_51_record: String,
    definition_xmt: NonNullXmt,
    attribute_definition: String,
}

#[cfg(test)]
impl From<ParasolidAttributeClassUse> for ParasolidAttributeClassUseWire {
    fn from(value: ParasolidAttributeClassUse) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            entity_51_record: value.entity_51_record,
            definition_xmt: value.definition_xmt,
            attribute_definition: value.attribute_definition,
        }
    }
}

/// Value-record family assigned to one declared Parasolid attribute field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ParasolidAttributeFieldValueKind {
    /// Type-82 integer values.
    UnsignedIntegers,
    /// Type-83 binary64 values.
    Doubles,
    /// Type-84 character values.
    String,
    /// Type-85 point values.
    Points,
    /// Type-86 vector values.
    Vectors,
    /// Type-87 axis values.
    Axes,
    /// Type-88 tag values.
    Tags,
    /// Type-89 direction values.
    Directions,
    /// Type-98 Unicode values.
    Unicode,
}

impl ParasolidAttributeFieldValueKind {
    pub(super) fn field_code(self) -> AttributeField {
        match self {
            Self::UnsignedIntegers => AttributeField::Integer,
            Self::Doubles => AttributeField::Real,
            Self::String => AttributeField::Character,
            Self::Points => AttributeField::Point,
            Self::Vectors => AttributeField::Vector,
            Self::Directions => AttributeField::Direction,
            Self::Axes => AttributeField::Axis,
            Self::Tags => AttributeField::Tag,
            Self::Unicode => AttributeField::Unicode,
        }
    }
}

/// One uniquely typed type-81 field reference joined to its type-80 declaration.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "FieldUseWire")]
pub(super) struct ParasolidAttributeFieldUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Resolved class relation for the attribute instance.
    pub(super) attribute_class_use: String,
    /// Type-81 attribute-instance record.
    pub(super) entity_51_record: String,
    /// Uniquely matched attribute definition.
    pub(super) attribute_definition: String,
    /// Position in the field declaration and complete type-81 reference lane.
    pub(super) position: FieldPosition,
    /// Resolved value-record family.
    pub(super) value_kind: ParasolidAttributeFieldValueKind,
    /// Type-81-to-value relation carrying this field.
    pub(super) value_use: String,
    /// Uniquely resolved value record.
    pub(super) value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static ATTRIBUTE_FIELD_USE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidAttributeFieldUse {
    fn clone(&self) -> Self {
        ATTRIBUTE_FIELD_USE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            stream_ordinal: self.stream_ordinal,
            attribute_class_use: self.attribute_class_use.clone(),
            entity_51_record: self.entity_51_record.clone(),
            attribute_definition: self.attribute_definition.clone(),
            position: self.position,
            value_kind: self.value_kind,
            value_use: self.value_use.clone(),
            value_record: self.value_record.clone(),
            inflated_offset: self.inflated_offset,
        }
    }
}

/// Resolved class of one topology-owned Parasolid attribute instance.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(test), derive(Clone))]
pub(super) struct ParasolidTopologyAttributeClassUse {
    /// Globally unique relation identity.
    pub(super) id: String,
    /// Owning topology-to-attribute relation.
    pub(super) topology_attribute_reference: String,
    /// Topology-owned type-81 attribute-instance record.
    pub(super) entity_51_record: String,
    /// Resolved class relation for the attribute instance.
    pub(super) attribute_class_use: String,
    /// Stream-local XMT of the matched type-80 definition.
    pub(super) definition_xmt: NonNullXmt,
    /// Uniquely matched attribute definition.
    pub(super) attribute_definition: String,
    /// Zero-based source stream ordinal.
    pub(super) stream_ordinal: u32,
    /// Offset of the owning type-81 record in the inflated stream.
    pub(super) inflated_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for ParasolidTopologyAttributeClassUse {
    fn clone(&self) -> Self {
        TOPOLOGY_ATTRIBUTE_CLASS_USE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            topology_attribute_reference: self.topology_attribute_reference.clone(),
            entity_51_record: self.entity_51_record.clone(),
            attribute_class_use: self.attribute_class_use.clone(),
            definition_xmt: self.definition_xmt,
            attribute_definition: self.attribute_definition.clone(),
            stream_ordinal: self.stream_ordinal,
            inflated_offset: self.inflated_offset,
        }
    }
}

#[derive(Serialize)]
struct ParasolidTopologyAttributeClassUseRef<'a> {
    id: &'a str,
    topology_attribute_reference: &'a str,
    entity_51_record: &'a str,
    attribute_class_use: &'a str,
    definition_xmt: NonNullXmt,
    attribute_definition: &'a str,
}

impl Serialize for ParasolidTopologyAttributeClassUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ParasolidTopologyAttributeClassUseRef {
            id: &self.id,
            topology_attribute_reference: &self.topology_attribute_reference,
            entity_51_record: &self.entity_51_record,
            attribute_class_use: &self.attribute_class_use,
            definition_xmt: self.definition_xmt,
            attribute_definition: &self.attribute_definition,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct ParasolidTopologyAttributeClassUseWire {
    id: String,
    topology_attribute_reference: String,
    entity_51_record: String,
    attribute_class_use: String,
    definition_xmt: NonNullXmt,
    attribute_definition: String,
}

#[cfg(test)]
impl From<ParasolidTopologyAttributeClassUse> for ParasolidTopologyAttributeClassUseWire {
    fn from(value: ParasolidTopologyAttributeClassUse) -> Self {
        Self {
            id: value.id,
            topology_attribute_reference: value.topology_attribute_reference,
            entity_51_record: value.entity_51_record,
            attribute_class_use: value.attribute_class_use,
            definition_xmt: value.definition_xmt,
            attribute_definition: value.attribute_definition,
        }
    }
}

/// Retain named attribute-class declarations from all Parasolid streams.
pub(super) fn parasolid_attribute_definitions(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidAttributeDefinition>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in ctx
        .admit_iter(streams, "NX attribute definition streams")?
        .enumerate()
    {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX attribute definition stream ordinal", 0, 1))?;
        let crate::parasolid::AttributeScan {
            records: scanned,
            slots: _scan_slots,
            payloads,
        } = crate::parasolid::attribute_definitions(ctx, &stream.inflated)?;
        payloads.commit()?;
        for definition in ctx.admit_iter(scanned, "NX attribute definitions")? {
            ctx.reserve_vec(&mut records, 1, "NX attribute definitions")?;
            let name = crate::printable_string::PrintableString::new(ctx.copy_retained_text(
                definition.name.as_str(),
                "retain NX attribute definition name",
            )?)
            .map_err(|message| retained_malformed(ctx, message))?;
            let id = parasolid_record_id(
                ctx,
                stream_ordinal,
                "attribute-definition",
                u32::from(definition.xmt),
            )?;
            records.push(ParasolidAttributeDefinition {
                id,
                stream_ordinal: ordinal,
                xmt: definition.xmt,
                next_definition_xmt: definition.next_definition_xmt,
                identifier_xmt: definition.identifier_xmt,
                identifier_inflated_offset: cadmpeg_core::decode::u64_from_index(
                    definition.identifier_offset,
                ),
                name,
                type_id: definition.type_id,
                action_codes: definition.action_codes,
                field_names_xmt: definition.field_names_xmt,
                legal_owner_flags: definition.legal_owner_flags,
                field_codes: definition.field_codes,
                inflated_offset: cadmpeg_core::decode::u64_from_index(definition.offset),
            });
        }
    }
    Ok(records)
}

/// Decode every counted type-99 attribute field-name record.
pub(super) fn parasolid_field_names_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidFieldNamesRecord>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in ctx
        .admit_iter(streams, "NX field names streams")?
        .enumerate()
    {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX field names stream ordinal", 0, 1))?;
        let crate::parasolid::AttributeScan {
            records: scanned,
            slots: _scan_slots,
            payloads,
        } = crate::parasolid::field_names_records(ctx, &stream.inflated)?;
        payloads.commit()?;
        for record in ctx.admit_iter(scanned, "NX field names records")? {
            ctx.reserve_vec(&mut records, 1, "NX field names records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "field-names",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.push(ParasolidFieldNamesRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                name_xmts: record.name_xmts,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
    }
    ctx.stable_sort_by(
        &mut records,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid records",
    )?;
    Ok(records)
}

/// Resolve complete type-80 field-name lists through type-99 and character records.
pub(super) fn parasolid_attribute_field_names(
    ctx: &DecodeContext<'_>,
    definitions: &[ParasolidAttributeDefinition],
    field_names: &[ParasolidFieldNamesRecord],
    strings: &[ParasolidEntity54StringRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Result<Vec<ParasolidAttributeFieldNames>, CodecError> {
    let (definitions_by_identity, _definitions_storage) = ctx.unique_index(
        definitions
            .iter()
            .map(|definition| ((definition.stream_ordinal, u32::from(definition.xmt)), ())),
        "NX attribute field definition index",
    )?;
    let (lists, _lists_storage) = ctx.unique_index(
        field_names
            .iter()
            .map(|list| ((list.stream_ordinal, u32::from(list.xmt)), list)),
        "NX attribute field list index",
    )?;
    let (names_by_xmt, _names_storage) = ctx.unique_index(
        ctx.admit_iter(strings, "NX attribute field name index")?
            .map(|string| {
                (
                    (string.stream_ordinal, u32::from(string.xmt)),
                    (string.id.as_str(), string.value.as_str()),
                )
            })
            .chain(
                ctx.admit_iter(unicode, "NX attribute field name index")?
                    .map(|value| {
                        (
                            (value.stream_ordinal, u32::from(value.xmt)),
                            (value.id.as_str(), value.value.as_str()),
                        )
                    }),
            ),
        "NX attribute field name index",
    )?;
    let mut relations = Vec::new();
    for definition in ctx.admit_iter(definitions, "resolve NX attribute field names")? {
        if !ctx
            .get_hash_map(
                &definitions_by_identity,
                &(definition.stream_ordinal, u32::from(definition.xmt)),
                "NX attribute field definition index",
            )?
            .is_some_and(Option::is_some)
        {
            continue;
        }
        let Some(field_names_xmt) = definition.field_names_xmt else {
            continue;
        };
        let Some(&Some(list)) = ctx.get_hash_map(
            &lists,
            &(definition.stream_ordinal, u32::from(field_names_xmt)),
            "NX attribute field list index",
        )?
        else {
            continue;
        };
        if list.name_xmts.as_slice().len() != definition.field_codes.len() {
            continue;
        }
        let name_of = |xmt: &NonNullXmt| -> Result<Option<(&str, &str)>, CodecError> {
            Ok(ctx
                .get_hash_map(
                    &names_by_xmt,
                    &(definition.stream_ordinal, u32::from(*xmt)),
                    "NX attribute field name index",
                )?
                .copied()
                .flatten())
        };
        if !ctx.all_by(
            list.name_xmts.as_slice(),
            |xmt| Ok(name_of(xmt)?.is_some()),
            "resolve NX attribute field names",
        )? {
            continue;
        }
        let mut resolved = Vec::new();
        for xmt in ctx.admit_iter(list.name_xmts.as_slice(), "NX attribute field names")? {
            let Some((value_record, name)) = name_of(xmt)? else {
                continue;
            };
            ctx.reserve_vec(&mut resolved, 1, "NX attribute field names")?;
            resolved.push(NamedField {
                value_record: ctx
                    .copy_retained_text(value_record, "NX entity 51 value use text")?,
                name: ctx.copy_retained_text(name, "NX entity 51 value use text")?,
            });
        }
        ctx.reserve_vec(&mut relations, 1, "NX attribute field name relations")?;
        let ordinal = usize::try_from(definition.stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX attribute field name stream ordinal", 0, 1))?;
        relations.push(ParasolidAttributeFieldNames {
            id: parasolid_record_id(
                ctx,
                ordinal,
                "attribute-field-names",
                u32::from(definition.xmt),
            )?,
            stream_ordinal: definition.stream_ordinal,
            attribute_definition: ctx
                .copy_retained_text(&definition.id, "NX entity 51 value use text")?,
            field_names_record: ctx.copy_retained_text(&list.id, "NX entity 51 value use text")?,
            fields: resolved,
        });
    }
    ctx.stable_sort_by(
        &mut relations,
        |value| &value.id,
        Ord::cmp,
        "sort NX attribute field name relations",
    )?;
    Ok(relations)
}

/// Retain complete typed rolling-ball blend records from all Parasolid streams.
pub(super) fn parasolid_blend_surface_records(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams,
) -> Result<Vec<ParasolidBlendSurfaceRecord>, CodecError> {
    per_parasolid_stream::<ParasolidBlendSurfaceRecord>(ctx, parsed)
}

impl ParasolidStreamRecords for ParasolidBlendSurfaceRecord {
    type Row = crate::topology::BlendSurface;
    type Record = ParasolidBlendSurfaceRecord;
    const ID_STEM: &'static str = "blend-surface-record";
    fn rows(view: &StreamView) -> &[Self::Row] {
        &view.blend_surfaces
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: &Self::Row) -> Self::Record {
        ParasolidBlendSurfaceRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            state: row.state,
            inflated_offset: cadmpeg_core::decode::u64_from_index(row.pos),
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Retain every non-null topology-to-attribute-list reference.
pub(super) fn parasolid_topology_attribute_list_references(
    ctx: &DecodeContext<'_>,
    parsed: &ParsedStreams,
    entity_records: &[ParasolidEntity51Record],
) -> Result<Vec<ParasolidTopologyAttributeListReference>, CodecError> {
    let (records_by_identity, _records_storage) = ctx.unique_index(
        entity_records.iter().map(|record| {
            (
                (record.stream_ordinal, u32::from(record.xmt)),
                record.id.as_str(),
            )
        }),
        "NX topology attribute list record index",
    )?;
    let mut references = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        ctx.charge_work(1, "scan NX topology attribute list streams")?;
        let graph = &stream.view_for_records().graph;
        for topology_type in TopologyAttributeKind::ALL {
            for node in graph.of_kind(topology_type.node_kind()) {
                ctx.charge_work(1, "scan NX topology attribute list references")?;
                let attribute_list_xmt = match topology_type {
                    TopologyAttributeKind::Shell => node
                        .shell_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Face => node
                        .face_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Loop => node
                        .loop_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Edge => node
                        .edge_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Fin => node
                        .fin_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                    TopologyAttributeKind::Vertex => node
                        .vertex_fields()
                        .and_then(|fields| fields.attributes.map(u32::from)),
                };
                let Some(attribute_list_xmt) = attribute_list_xmt.filter(|value| *value > 1) else {
                    continue;
                };
                let Some(inflated_offset) = node.attribute_field_offset() else {
                    continue;
                };
                let ordinal = u32::try_from(stream_ordinal).map_err(|_| {
                    ctx.refuse_codec_limit("NX topology attribute stream ordinal", 0, 1)
                })?;
                ctx.reserve_vec(&mut references, 1, "NX topology attribute list references")?;
                let digits = |value: u64| {
                    value
                        .checked_ilog10()
                        .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
                };
                let length = "nx:s"
                    .len()
                    .checked_add(digits(cadmpeg_core::decode::u64_from_index(stream_ordinal)))
                    .and_then(|length| {
                        length.checked_add(":topology-attribute-list-reference#".len())
                    })
                    .and_then(|length| length.checked_add(digits(u64::from(topology_type.code()))))
                    .and_then(|length| length.checked_add(1 + digits(u64::from(node.xmt()))))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "NX topology attribute list reference identity",
                            0,
                            1,
                        )
                    })?;
                let mut id =
                    ctx.retained_string(length, "NX topology attribute list reference identity")?;
                write!(
                    &mut id,
                    "nx:s{stream_ordinal}:topology-attribute-list-reference#{}-{}",
                    topology_type.code(),
                    node.xmt()
                )
                .map_err(|_| {
                    ctx.refuse_codec_limit(
                        "write NX topology attribute list reference identity",
                        0,
                        1,
                    )
                })?;
                let attribute_list_record = match ctx.get_hash_map(
                    &records_by_identity,
                    &(ordinal, attribute_list_xmt),
                    "NX topology attribute list record index",
                )? {
                    Some(Some(record)) => {
                        Some(ctx.copy_retained_text(record, "NX entity 51 value use text")?)
                    }
                    _ => None,
                };
                references.push(ParasolidTopologyAttributeListReference {
                    id,
                    stream_ordinal: ordinal,
                    topology_type,
                    topology_xmt: node.xmt(),
                    attribute_list_xmt,
                    attribute_list_record,
                    inflated_offset: cadmpeg_core::decode::u64_from_index(inflated_offset),
                });
            }
        }
    }
    Ok(references)
}

/// Decode every framed type-81 entity/attribute-list record.
pub(super) fn parasolid_entity_51_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
) -> Result<Vec<ParasolidEntity51Record>, CodecError> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in ctx.admit_iter(streams, "NX entity 51 streams")?.enumerate() {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX entity 51 stream ordinal", 0, 1))?;
        let crate::parasolid::AttributeScan {
            records: scanned,
            slots: _scan_slots,
            payloads,
        } = crate::parasolid::entity_51_records(ctx, &stream.inflated)?;
        payloads.commit()?;
        for record in ctx.admit_iter(scanned, "NX entity 51 records")? {
            ctx.reserve_vec(&mut records, 1, "NX entity 51 records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-51",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.push(ParasolidEntity51Record {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                sequence: record.sequence,
                definition_xmt: record.definition_xmt,
                leading_references: record.leading_references,
                trailing_references: record.trailing_references,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
    }
    ctx.stable_sort_by(
        &mut records,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid records",
    )?;
    Ok(records)
}

pub(super) fn parasolid_entity_value_records(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
    deltas_records: &[ParasolidDeltasRecord],
) -> Result<ParasolidEntityValueRecords, CodecError> {
    let mut records = ParasolidEntityValueRecords {
        integers: Vec::new(),
        doubles: Vec::new(),
        strings: Vec::new(),
        vectors: Vec::new(),
        axes: Vec::new(),
        tags: Vec::new(),
        unicode: Vec::new(),
    };
    let mut deltas_offsets = BTreeMap::<u32, Vec<usize>>::new();
    let mut deltas_offsets_storage = ctx.reserve_scoped(0, "NX value record owner offsets")?;
    for record in ctx.admit_iter(deltas_records, "NX value record owner offsets")? {
        if !matches!(
            record.family,
            RecordFamily::Entity52
                | RecordFamily::Entity53
                | RecordFamily::Entity54
                | RecordFamily::Entity55
                | RecordFamily::Entity56
                | RecordFamily::Entity57
                | RecordFamily::Entity58
                | RecordFamily::Entity59
                | RecordFamily::Entity62
        ) {
            continue;
        }
        let Ok(offset) = usize::try_from(record.inflated_offset) else {
            continue;
        };
        deltas_offsets_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut deltas_offsets,
                record.stream_ordinal,
                offset,
                "NX value record owner offsets",
                "NX value record owner offsets",
            )
        })?;
    }
    for (stream_ordinal, stream) in ctx
        .admit_iter(streams, "NX value record streams")?
        .enumerate()
    {
        let ordinal = u32::try_from(stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX value record stream ordinal", 0, 1))?;
        let (owned_offsets, offsets_guard) = match stream.kind() {
            StreamKind::Deltas => (
                ctx.remove_btree_map(
                    &mut deltas_offsets,
                    &ordinal,
                    "NX value record owner offsets",
                )?
                .unwrap_or_default(),
                None,
            ),
            StreamKind::Partition | StreamKind::Plain => {
                let (offsets, guard) =
                    crate::parasolid::referenced_value_record_offsets(ctx, &stream.inflated)?;
                (offsets, Some(guard))
            }
            StreamKind::Preview => continue,
        };
        let values = crate::parasolid::value_records::entity_value_records_at(
            ctx,
            &stream.inflated,
            owned_offsets,
        )?;
        drop(offsets_guard);
        for record in ctx.admit_iter(values.integers, "NX Parasolid value records")? {
            ctx.reserve_vec(&mut records.integers, 1, "NX Parasolid value records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-52-integers",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.integers.push(ParasolidEntity52IntegerRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in ctx.admit_iter(values.doubles, "NX Parasolid value records")? {
            ctx.reserve_vec(&mut records.doubles, 1, "NX Parasolid value records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-53-doubles",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.doubles.push(ParasolidEntity53DoubleRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in ctx.admit_iter(values.strings, "NX Parasolid value records")? {
            ctx.reserve_vec(&mut records.strings, 1, "NX Parasolid value records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-54-string",
                u32::from(record.xmt),
                record.offset,
            )?;
            let value = crate::printable_string::PrintableString::new(
                ctx.copy_retained_text(record.value.as_str(), "retain NX Parasolid string value")?,
            )
            .map_err(|message| retained_malformed(ctx, message))?;
            records.strings.push(ParasolidEntity54StringRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        let mut retain_vector = |kind,
                                 family: &'static str,
                                 xmt,
                                 offset,
                                 byte_len,
                                 values|
         -> Result<(), CodecError> {
            ctx.reserve_vec(&mut records.vectors, 1, "NX Parasolid value records")?;
            let id =
                parasolid_offset_record_id(ctx, stream_ordinal, family, u32::from(xmt), offset)?;
            records.vectors.push(ParasolidEntityVectorRecord {
                id,
                stream_ordinal: ordinal,
                kind,
                xmt,
                values,
                byte_len: cadmpeg_core::decode::u64_from_index(byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(offset),
            });
            Ok(())
        };
        for record in ctx.admit_iter(values.points, "NX Parasolid value records")? {
            retain_vector(
                ParasolidVectorValueKind::Points,
                "entity-55-points",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            )?;
        }
        for record in ctx.admit_iter(values.vectors, "NX Parasolid value records")? {
            retain_vector(
                ParasolidVectorValueKind::Vectors,
                "entity-56-vectors",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            )?;
        }
        for record in ctx.admit_iter(values.directions, "NX Parasolid value records")? {
            retain_vector(
                ParasolidVectorValueKind::Directions,
                "entity-59-directions",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            )?;
        }
        for record in ctx.admit_iter(values.axes, "NX Parasolid value records")? {
            ctx.reserve_vec(&mut records.axes, 1, "NX Parasolid value records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-57-axes",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.axes.push(ParasolidEntity57AxisRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in ctx.admit_iter(values.tags, "NX Parasolid value records")? {
            ctx.reserve_vec(&mut records.tags, 1, "NX Parasolid value records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-58-tags",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.tags.push(ParasolidEntity58TagRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                values: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
        for record in ctx.admit_iter(values.unicode, "NX Parasolid value records")? {
            ctx.reserve_vec(&mut records.unicode, 1, "NX Parasolid value records")?;
            let id = parasolid_offset_record_id(
                ctx,
                stream_ordinal,
                "entity-62-unicode",
                u32::from(record.xmt),
                record.offset,
            )?;
            records.unicode.push(ParasolidEntity62UnicodeRecord {
                id,
                stream_ordinal: ordinal,
                xmt: record.xmt,
                value: record.value,
                byte_len: cadmpeg_core::decode::u64_from_index(record.byte_len),
                inflated_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            });
        }
    }
    ctx.stable_sort_by(
        &mut records.integers,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value integers",
    )?;
    ctx.stable_sort_by(
        &mut records.doubles,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value doubles",
    )?;
    ctx.stable_sort_by(
        &mut records.strings,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value strings",
    )?;
    ctx.stable_sort_by(
        &mut records.vectors,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value vectors",
    )?;
    ctx.stable_sort_by(
        &mut records.axes,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value axes",
    )?;
    ctx.stable_sort_by(
        &mut records.tags,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value tags",
    )?;
    ctx.stable_sort_by(
        &mut records.unicode,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid value unicode",
    )?;
    Ok(records)
}

fn entity_51_use_id(
    ctx: &DecodeContext<'_>,
    stem: &'static str,
    entity: &ParasolidEntity51Record,
    reference_ordinal: u32,
) -> Result<String, CodecError> {
    let digits = |value: u64| {
        value
            .checked_ilog10()
            .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
    };
    let length = "nx:s"
        .len()
        .checked_add(digits(u64::from(entity.stream_ordinal)))
        .and_then(|length| length.checked_add(1 + stem.len() + 1))
        .and_then(|length| length.checked_add(digits(u64::from(u32::from(entity.xmt)))))
        .and_then(|length| length.checked_add(1 + digits(entity.inflated_offset)))
        .and_then(|length| length.checked_add(1 + digits(u64::from(reference_ordinal))))
        .ok_or_else(|| ctx.refuse_codec_limit("NX entity 51 value use identity", 0, 1))?;
    let mut id = ctx.retained_string(length, "NX entity 51 value use identity")?;
    write!(
        &mut id,
        "nx:s{}:{stem}#{}-{}-{reference_ordinal}",
        entity.stream_ordinal,
        u32::from(entity.xmt),
        entity.inflated_offset
    )
    .map_err(|_| ctx.refuse_codec_limit("write NX entity 51 value use identity", 0, 1))?;
    Ok(id)
}

fn sort_entity_51_uses<T>(
    ctx: &DecodeContext<'_>,
    uses: &mut [T],
    id: impl Fn(&T) -> &str,
) -> Result<(), CodecError> {
    ctx.stable_sort_by(
        uses,
        |value| id(value),
        Ord::cmp,
        "sort NX entity 51 value uses",
    )?;
    Ok(())
}

pub(super) fn parasolid_entity_51_numeric_uses(
    ctx: &DecodeContext<'_>,
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Result<Vec<ParasolidEntity51NumericUse>, CodecError> {
    let (values, _values_storage) = ctx.unique_index(
        ctx.admit_iter(integers, "NX entity 51 value identity index")?
            .map(|record| {
                (
                    (record.stream_ordinal, u32::from(record.xmt)),
                    (
                        ParasolidEntity51NumericKind::UnsignedIntegers,
                        record.xmt,
                        record.id.as_str(),
                    ),
                )
            })
            .chain(
                ctx.admit_iter(doubles, "NX entity 51 value identity index")?
                    .map(|record| {
                        (
                            (record.stream_ordinal, u32::from(record.xmt)),
                            (
                                ParasolidEntity51NumericKind::Doubles,
                                record.xmt,
                                record.id.as_str(),
                            ),
                        )
                    }),
            ),
        "NX entity 51 value identity index",
    )?;
    let mut uses = Vec::new();
    for entity in ctx.admit_iter(entities, "NX entity 51 value uses")? {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some(&Some((kind, target_xmt, value_record))) = ctx.get_hash_map(
                &values,
                &(entity.stream_ordinal, referenced_xmt),
                "NX entity 51 value identity index",
            )?
            else {
                continue;
            };
            ctx.push_vec(
                &mut uses,
                ParasolidEntity51NumericUse {
                    id: entity_51_use_id(ctx, "entity-51-numeric-use", entity, reference_ordinal)?,
                    stream_ordinal: entity.stream_ordinal,
                    entity_51_record: ctx
                        .copy_retained_text(&entity.id, "NX entity 51 value use text")?,
                    position,
                    referenced_xmt: target_xmt,
                    kind,
                    value_record: ctx
                        .copy_retained_text(value_record, "NX entity 51 value use text")?,
                    inflated_offset: entity.inflated_offset,
                },
                "NX entity 51 value uses",
            )?;
        }
    }
    sort_entity_51_uses(ctx, &mut uses, |value| &value.id)?;
    Ok(uses)
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub(super) fn parasolid_entity_51_string_uses(
    ctx: &DecodeContext<'_>,
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Result<Vec<ParasolidEntity51StringUse>, CodecError> {
    let (strings_by_identity, _strings_storage) = ctx.unique_index(
        strings
            .iter()
            .map(|string| ((string.stream_ordinal, u32::from(string.xmt)), string)),
        "NX entity 51 value identity index",
    )?;
    let mut uses = Vec::new();
    for entity in ctx.admit_iter(entities, "NX entity 51 value uses")? {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some(&Some(string)) = ctx.get_hash_map(
                &strings_by_identity,
                &(entity.stream_ordinal, referenced_xmt),
                "NX entity 51 value identity index",
            )?
            else {
                continue;
            };
            ctx.push_vec(
                &mut uses,
                ParasolidEntity51StringUse {
                    id: entity_51_use_id(ctx, "entity-51-string-use", entity, reference_ordinal)?,
                    stream_ordinal: entity.stream_ordinal,
                    entity_51_record: ctx
                        .copy_retained_text(&entity.id, "NX entity 51 value use text")?,
                    position,
                    referenced_xmt: string.xmt,
                    string_record: ctx
                        .copy_retained_text(&string.id, "NX entity 51 value use text")?,
                    inflated_offset: entity.inflated_offset,
                },
                "NX entity 51 value uses",
            )?;
        }
    }
    sort_entity_51_uses(ctx, &mut uses, |value| &value.id)?;
    Ok(uses)
}

/// Join type-81 reference slots to unique same-stream structured value records.
pub(super) fn parasolid_entity_51_structured_uses(
    ctx: &DecodeContext<'_>,
    entities: &[ParasolidEntity51Record],
    vectors: &[ParasolidEntityVectorRecord],
    axes: &[ParasolidEntity57AxisRecord],
    tags: &[ParasolidEntity58TagRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Result<Vec<ParasolidEntity51StructuredUse>, CodecError> {
    fn structured(
        kind: StructuredValueKind,
        stream_ordinal: u32,
        xmt: NonNullXmt,
        id: &str,
    ) -> ((u32, u32), (StructuredValueKind, NonNullXmt, &str)) {
        ((stream_ordinal, u32::from(xmt)), (kind, xmt, id))
    }
    let (values, _values_storage) = ctx.unique_index(
        ctx.admit_iter(vectors, "NX entity 51 value identity index")?
            .map(|record| {
                let kind = match record.kind {
                    ParasolidVectorValueKind::Points => StructuredValueKind::Points,
                    ParasolidVectorValueKind::Vectors => StructuredValueKind::Vectors,
                    ParasolidVectorValueKind::Directions => StructuredValueKind::Directions,
                };
                structured(kind, record.stream_ordinal, record.xmt, record.id.as_str())
            })
            .chain(
                ctx.admit_iter(axes, "NX entity 51 value identity index")?
                    .map(|record| {
                        structured(
                            StructuredValueKind::Axes,
                            record.stream_ordinal,
                            record.xmt,
                            record.id.as_str(),
                        )
                    }),
            )
            .chain(
                ctx.admit_iter(tags, "NX entity 51 value identity index")?
                    .map(|record| {
                        structured(
                            StructuredValueKind::Tags,
                            record.stream_ordinal,
                            record.xmt,
                            record.id.as_str(),
                        )
                    }),
            )
            .chain(
                ctx.admit_iter(unicode, "NX entity 51 value identity index")?
                    .map(|record| {
                        structured(
                            StructuredValueKind::Unicode,
                            record.stream_ordinal,
                            record.xmt,
                            record.id.as_str(),
                        )
                    }),
            ),
        "NX entity 51 value identity index",
    )?;
    let mut uses = Vec::new();
    for entity in ctx.admit_iter(entities, "NX entity 51 value uses")? {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some(&Some((kind, target_xmt, value_record))) = ctx.get_hash_map(
                &values,
                &(entity.stream_ordinal, referenced_xmt),
                "NX entity 51 value identity index",
            )?
            else {
                continue;
            };
            ctx.push_vec(
                &mut uses,
                ParasolidEntity51StructuredUse {
                    id: entity_51_use_id(
                        ctx,
                        "entity-51-structured-use",
                        entity,
                        reference_ordinal,
                    )?,
                    stream_ordinal: entity.stream_ordinal,
                    entity_51_record: ctx
                        .copy_retained_text(&entity.id, "NX entity 51 value use text")?,
                    position,
                    referenced_xmt: target_xmt,
                    kind,
                    value_record: ctx
                        .copy_retained_text(value_record, "NX entity 51 value use text")?,
                    inflated_offset: entity.inflated_offset,
                },
                "NX entity 51 value uses",
            )?;
        }
    }
    sort_entity_51_uses(ctx, &mut uses, |value| &value.id)?;
    Ok(uses)
}

/// Resolve topology-owned attribute instances through their type-80 definition.
pub(super) fn parasolid_topology_attribute_class_uses(
    ctx: &DecodeContext<'_>,
    topology_references: &[ParasolidTopologyAttributeListReference],
    entity_records: &[ParasolidEntity51Record],
    class_uses: &[ParasolidAttributeClassUse],
) -> Result<Vec<ParasolidTopologyAttributeClassUse>, CodecError> {
    let (records_by_identity, _identity_storage) = ctx.unique_index(
        entity_records
            .iter()
            .map(|record| ((record.stream_ordinal, u32::from(record.xmt)), record)),
        "NX topology attribute entity index",
    )?;
    let mut records_by_owner = HashMap::<(u32, u32), Vec<&ParasolidEntity51Record>>::new();
    let mut owner_storage = ctx.reserve_scoped(0, "NX topology attribute owner index")?;
    for record in ctx.admit_iter(entity_records, "NX topology attribute owner index")? {
        let owner_xmt = record.leading_references[0];
        if owner_xmt > 1 {
            owner_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut records_by_owner,
                    (record.stream_ordinal, owner_xmt),
                    record,
                    "NX topology attribute owner index",
                    "NX topology attribute owner members",
                )
            })?;
        }
    }
    let (class_uses_by_entity, _class_storage) = ctx.unique_index(
        class_uses
            .iter()
            .map(|class_use| (class_use.entity_51_record.as_str(), class_use)),
        "NX topology attribute class index",
    )?;
    let mut uses = Vec::new();
    for reference in ctx.admit_iter(topology_references, "NX topology attribute class uses")? {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some(&Some(head)) = ctx.get_hash_map(
            &records_by_identity,
            &(reference.stream_ordinal, reference.attribute_list_xmt),
            "NX topology attribute entity index",
        )?
        else {
            continue;
        };
        if !ctx.equal(
            head.id.as_str(),
            entity_id,
            "NX topology attribute entity index",
        )? || head.leading_references[0] != reference.topology_xmt
        {
            continue;
        }

        let Some(members) = ctx.get_hash_map(
            &records_by_owner,
            &(reference.stream_ordinal, reference.topology_xmt),
            "NX topology attribute owner index",
        )?
        else {
            continue;
        };

        let mut member_xmt_counts = HashMap::<u32, usize>::new();
        let mut count_storage = ctx.reserve_scoped(0, "NX topology attribute member XMT counts")?;
        for member in ctx.admit_iter(members, "NX topology attribute member XMT counts")? {
            let member_key = u32::from(member.xmt);
            if let Some(count) = ctx.get_mut_hash_map(
                &mut member_xmt_counts,
                &member_key,
                "NX topology attribute member XMT counts",
            )? {
                *count = count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("count NX topology attribute member XMT", 0, 1)
                })?;
            } else {
                count_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut member_xmt_counts,
                        member_key,
                        1,
                        "NX topology attribute member XMT counts",
                    )
                })?;
            }
        }
        for member in ctx.admit_iter(members, "NX topology attribute class uses")? {
            let Some(&Some(class_use)) = ctx.get_hash_map(
                &class_uses_by_entity,
                member.id.as_str(),
                "NX topology attribute class index",
            )?
            else {
                continue;
            };
            let digits = |value: u64| {
                value
                    .checked_ilog10()
                    .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
            };
            let mut length = "nx:s"
                .len()
                .checked_add(digits(u64::from(reference.stream_ordinal)))
                .and_then(|length| length.checked_add(":topology-attribute-class-use#".len()))
                .and_then(|length| {
                    length.checked_add(digits(u64::from(reference.topology_type.code())))
                })
                .and_then(|length| {
                    length.checked_add(1 + digits(u64::from(reference.topology_xmt)))
                })
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("NX topology attribute class identity", 0, 1)
                })?;
            let suffix = !ctx.equal(
                member.id.as_str(),
                head.id.as_str(),
                "NX topology attribute class identity",
            )?;
            let offset_suffix = suffix
                && ctx.get_hash_map(
                    &member_xmt_counts,
                    &u32::from(member.xmt),
                    "NX topology attribute member XMT counts",
                )? != Some(&1);
            if suffix {
                length = length
                    .checked_add(1 + digits(u64::from(u32::from(member.xmt))))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX topology attribute class identity", 0, 1)
                    })?;
            }
            if offset_suffix {
                length = length
                    .checked_add(1 + digits(member.inflated_offset))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX topology attribute class identity", 0, 1)
                    })?;
            }
            let mut id = ctx.retained_string(length, "NX topology attribute class identity")?;
            write!(
                &mut id,
                "nx:s{}:topology-attribute-class-use#{}-{}",
                reference.stream_ordinal,
                reference.topology_type.code(),
                reference.topology_xmt
            )
            .map_err(|_| {
                ctx.refuse_codec_limit("write NX topology attribute class identity", 0, 1)
            })?;
            if suffix {
                write!(&mut id, "-{}", u32::from(member.xmt)).map_err(|_| {
                    ctx.refuse_codec_limit("write NX topology attribute class identity", 0, 1)
                })?;
            }
            if offset_suffix {
                write!(&mut id, "-{}", member.inflated_offset).map_err(|_| {
                    ctx.refuse_codec_limit("write NX topology attribute class identity", 0, 1)
                })?;
            }
            ctx.reserve_vec(&mut uses, 1, "NX topology attribute class uses")?;
            uses.push(ParasolidTopologyAttributeClassUse {
                stream_ordinal: reference.stream_ordinal,
                inflated_offset: member.inflated_offset,
                id,
                topology_attribute_reference: ctx
                    .copy_retained_text(&reference.id, "NX entity 51 value use text")?,
                entity_51_record: ctx.copy_retained_text(
                    &class_use.entity_51_record,
                    "NX entity 51 value use text",
                )?,
                attribute_class_use: ctx
                    .copy_retained_text(&class_use.id, "NX entity 51 value use text")?,
                definition_xmt: class_use.definition_xmt,
                attribute_definition: ctx.copy_retained_text(
                    &class_use.attribute_definition,
                    "NX entity 51 value use text",
                )?,
            });
        }
    }
    ctx.stable_sort_by(
        &mut uses,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid uses",
    )?;
    Ok(uses)
}

/// Resolve every type-81 attribute instance through its type-80 definition reference.
pub(super) fn parasolid_attribute_class_uses(
    ctx: &DecodeContext<'_>,
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Result<Vec<ParasolidAttributeClassUse>, CodecError> {
    let (definitions_by_identity, _definitions_storage) = ctx.unique_index(
        definitions.iter().map(|definition| {
            (
                (definition.stream_ordinal, u32::from(definition.xmt)),
                definition,
            )
        }),
        "NX attribute class definition index",
    )?;
    let mut uses = Vec::new();
    for entity in ctx.admit_iter(entities, "NX attribute class uses")? {
        let Some(&Some(definition)) = ctx.get_hash_map(
            &definitions_by_identity,
            &(entity.stream_ordinal, entity.definition_xmt),
            "NX attribute class definition index",
        )?
        else {
            continue;
        };
        ctx.reserve_vec(&mut uses, 1, "NX attribute class uses")?;
        let digits = |value: u64| {
            value
                .checked_ilog10()
                .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
        };
        let length = "nx:s"
            .len()
            .checked_add(digits(u64::from(entity.stream_ordinal)))
            .and_then(|length| length.checked_add(":attribute-class-use#".len()))
            .and_then(|length| length.checked_add(digits(u64::from(u32::from(entity.xmt)))))
            .and_then(|length| length.checked_add(1 + digits(entity.inflated_offset)))
            .ok_or_else(|| ctx.refuse_codec_limit("NX attribute class use identity", 0, 1))?;
        let mut id = ctx.retained_string(length, "NX attribute class use identity")?;
        write!(
            &mut id,
            "nx:s{}:attribute-class-use#{}-{}",
            entity.stream_ordinal,
            u32::from(entity.xmt),
            entity.inflated_offset
        )
        .map_err(|_| ctx.refuse_codec_limit("write NX attribute class use identity", 0, 1))?;
        uses.push(ParasolidAttributeClassUse {
            inflated_offset: entity.inflated_offset,
            id,
            stream_ordinal: entity.stream_ordinal,
            entity_51_record: ctx.copy_retained_text(&entity.id, "NX entity 51 value use text")?,
            definition_xmt: definition.xmt,
            attribute_definition: ctx
                .copy_retained_text(&definition.id, "NX entity 51 value use text")?,
        });
    }
    ctx.stable_sort_by(
        &mut uses,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid uses",
    )?;
    Ok(uses)
}

/// Assign uniquely resolved attribute values to declared type-80 fields.
pub(super) fn parasolid_attribute_field_uses(
    ctx: &DecodeContext<'_>,
    class_uses: &[ParasolidAttributeClassUse],
    definitions: &[ParasolidAttributeDefinition],
    numeric_uses: &[ParasolidEntity51NumericUse],
    string_uses: &[ParasolidEntity51StringUse],
    structured_uses: &[ParasolidEntity51StructuredUse],
) -> Result<Vec<ParasolidAttributeFieldUse>, CodecError> {
    let (classes, _classes_storage) = ctx.unique_index(
        class_uses
            .iter()
            .map(|class_use| (class_use.entity_51_record.as_str(), class_use)),
        "NX attribute field class index",
    )?;
    let (definitions_by_id, _definitions_storage) = ctx.unique_index(
        definitions
            .iter()
            .map(|definition| (definition.id.as_str(), definition)),
        "NX attribute field definition index",
    )?;
    let mut candidates = BTreeMap::<(&str, FieldPosition), Vec<_>>::new();
    let mut candidates_storage = ctx.reserve_scoped(0, "NX attribute field candidates")?;
    for numeric_use in ctx.admit_iter(numeric_uses, "NX attribute field candidates")? {
        let value_kind = match numeric_use.kind {
            ParasolidEntity51NumericKind::UnsignedIntegers => {
                ParasolidAttributeFieldValueKind::UnsignedIntegers
            }
            ParasolidEntity51NumericKind::Doubles => ParasolidAttributeFieldValueKind::Doubles,
        };
        candidates_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut candidates,
                (numeric_use.entity_51_record.as_str(), numeric_use.position),
                (
                    numeric_use.stream_ordinal,
                    value_kind,
                    numeric_use.id.as_str(),
                    numeric_use.value_record.as_str(),
                    numeric_use.inflated_offset,
                ),
                "NX attribute field candidate index",
                "NX attribute field candidate members",
            )
        })?;
    }
    for string_use in ctx.admit_iter(string_uses, "NX attribute field candidates")? {
        candidates_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut candidates,
                (string_use.entity_51_record.as_str(), string_use.position),
                (
                    string_use.stream_ordinal,
                    ParasolidAttributeFieldValueKind::String,
                    string_use.id.as_str(),
                    string_use.string_record.as_str(),
                    string_use.inflated_offset,
                ),
                "NX attribute field candidate index",
                "NX attribute field candidate members",
            )
        })?;
    }
    for structured_use in ctx.admit_iter(structured_uses, "NX attribute field candidates")? {
        candidates_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut candidates,
                (
                    structured_use.entity_51_record.as_str(),
                    structured_use.position,
                ),
                (
                    structured_use.stream_ordinal,
                    structured_use.kind.into(),
                    structured_use.id.as_str(),
                    structured_use.value_record.as_str(),
                    structured_use.inflated_offset,
                ),
                "NX attribute field candidate index",
                "NX attribute field candidate members",
            )
        })?;
    }
    let mut uses = Vec::new();
    for (&(entity_51_record, position), candidates) in
        ctx.admit_iter(&candidates, "NX attribute field join work")?
    {
        let [(stream_ordinal, value_kind, value_use, value_record, inflated_offset)] =
            *candidates.as_slice()
        else {
            continue;
        };
        let Some(&Some(class_use)) =
            ctx.get_hash_map(&classes, entity_51_record, "NX attribute field class index")?
        else {
            continue;
        };
        if class_use.stream_ordinal != stream_ordinal {
            continue;
        }
        let Some(&Some(definition)) = ctx.get_hash_map(
            &definitions_by_id,
            class_use.attribute_definition.as_str(),
            "NX attribute field definition index",
        )?
        else {
            continue;
        };
        let field_ordinal = position.field_ordinal();
        let Some(&field_code) = definition
            .field_codes
            .get(cadmpeg_core::decode::index_from_u32(field_ordinal))
        else {
            continue;
        };
        if field_code != value_kind.field_code() {
            continue;
        }
        let Some((_, class_key)) =
            ctx.rsplit_once(&class_use.id, "#", "NX attribute field use identity")?
        else {
            continue;
        };
        let digits = |value: u64| {
            value
                .checked_ilog10()
                .map_or(1, |count| cadmpeg_core::decode::index_from_u32(count) + 1)
        };
        let id_len = "nx:s"
            .len()
            .checked_add(digits(u64::from(stream_ordinal)))
            .and_then(|length| length.checked_add(":attribute-field-use#".len()))
            .and_then(|length| length.checked_add(class_key.len()))
            .and_then(|length| length.checked_add(1 + digits(u64::from(field_ordinal))))
            .ok_or_else(|| ctx.refuse_codec_limit("NX attribute field use identity", 0, 1))?;
        let mut id = ctx.retained_string(id_len, "NX attribute field use identity")?;
        write!(
            &mut id,
            "nx:s{stream_ordinal}:attribute-field-use#{class_key}-{field_ordinal}"
        )
        .map_err(|_| ctx.refuse_codec_limit("write NX attribute field use identity", 0, 1))?;
        ctx.reserve_vec(&mut uses, 1, "NX attribute field uses")?;
        uses.push(ParasolidAttributeFieldUse {
            id,
            stream_ordinal,
            attribute_class_use: ctx
                .copy_retained_text(&class_use.id, "NX entity 51 value use text")?,
            entity_51_record: ctx
                .copy_retained_text(entity_51_record, "NX entity 51 value use text")?,
            attribute_definition: ctx.copy_retained_text(
                &class_use.attribute_definition,
                "NX entity 51 value use text",
            )?,
            position,
            value_kind,
            value_use: ctx.copy_retained_text(value_use, "NX entity 51 value use text")?,
            value_record: ctx.copy_retained_text(value_record, "NX entity 51 value use text")?,
            inflated_offset,
        });
    }
    ctx.stable_sort_by(
        &mut uses,
        |value| &value.id,
        Ord::cmp,
        "sort NX Parasolid uses",
    )?;
    Ok(uses)
}

/// Whether a concrete topology-owned attribute field lacks its exact value relation.
pub(super) fn parasolid_topology_attribute_fields_have_untransferred_values(
    ctx: &DecodeContext<'_>,
    definitions: &[ParasolidAttributeDefinition],
    entities: &[ParasolidEntity51Record],
    field_uses: &[ParasolidAttributeFieldUse],
    topology_class_uses: &[ParasolidTopologyAttributeClassUse],
) -> Result<bool, CodecError> {
    let (definitions_by_id, _definitions_storage) = ctx.unique_index(
        definitions
            .iter()
            .map(|definition| (definition.id.as_str(), definition)),
        "NX topology attribute definition index",
    )?;
    let (fields_by_identity, _fields_storage) = ctx.unique_index(
        field_uses.iter().map(|field_use| {
            (
                (
                    field_use.entity_51_record.as_str(),
                    field_use.position.field_ordinal(),
                ),
                (),
            )
        }),
        "NX topology attribute field index",
    )?;
    let (entities_by_id, _entities_storage) = ctx.unique_index(
        entities.iter().map(|entity| (entity.id.as_str(), entity)),
        "NX topology attribute entity index",
    )?;
    ctx.any_by(
        topology_class_uses,
        |topology_class_use| {
            let Some(&Some(entity)) = ctx.get_hash_map(
                &entities_by_id,
                topology_class_use.entity_51_record.as_str(),
                "NX topology attribute entity index",
            )?
            else {
                return Ok(true);
            };
            let Some(&Some(definition)) = ctx.get_hash_map(
                &definitions_by_id,
                topology_class_use.attribute_definition.as_str(),
                "NX topology attribute definition index",
            )?
            else {
                return Ok(true);
            };
            let mut field_ordinal = 0_usize;
            ctx.any_by(
                &definition.field_codes,
                |field_code| {
                    let ordinal = field_ordinal;
                    field_ordinal += 1;
                    // Field code 0 is ignored. Pointer fields (code 9) are always
                    // transmitted empty and therefore have no value relation.
                    if matches!(
                        field_code,
                        AttributeField::Ignored | AttributeField::Pointer
                    ) {
                        return Ok(false);
                    }
                    let Some(&referenced_xmt) = entity.trailing_references.values().get(ordinal)
                    else {
                        return Ok(true);
                    };
                    if referenced_xmt == 1 {
                        return Ok(false);
                    }
                    let Ok(ordinal) = u32::try_from(ordinal) else {
                        return Ok(true);
                    };
                    Ok(!ctx
                        .get_hash_map(
                            &fields_by_identity,
                            &(entity.id.as_str(), ordinal),
                            "NX topology attribute field index",
                        )?
                        .is_some_and(Option::is_some))
                },
                "NX topology attribute field validation work",
            )
        },
        "NX topology attribute field validation work",
    )
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_group_selector, GroupSelector, "group_selector");
cadmpeg_core::named_optional_field!(
    deserialize_group_linked_reference_status,
    GroupReferenceStatus,
    "group_linked_reference_status"
);
cadmpeg_core::named_optional_field!(
    deserialize_attribute_list_record,
    String,
    "attribute_list_record"
);
