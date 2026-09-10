// SPDX-License-Identifier: Apache-2.0
//! Parasolid source-record extractors and their record types.

use crate::framing::xmt_reference::XmtTarget;
use crate::parasolid::name_references::NameReferences;
use crate::parasolid::{Stream, StreamKind};
use crate::topology::blend_surface_state::BlendSurfaceState;
use crate::topology::offset_surface_state::OffsetSurfaceState;
use serde::{Deserialize, Serialize};

use crate::deltas::census::Census;
use crate::deltas::record_family::RecordFamily;
use crate::intersection::finite_point::FinitePoint;
use crate::parasolid::attribute_action::AttributeAction;
use crate::parasolid::attribute_field::AttributeField;
use std::num::NonZeroU32;

pub(crate) mod structured_value_kind;
use structured_value_kind::StructuredValueKind;

mod field_use_wire;
use field_use_wire::FieldUseWire;

pub(crate) mod topology_attribute_kind;
use topology_attribute_kind::TopologyAttributeKind;

mod entity51_wire;
use crate::parasolid::counted_values::CountedValues;
use crate::parasolid::entity_references::{EntityReferences, FieldPosition};
use crate::parasolid::unicode_value::UnicodeValue;
use crate::printable_string::PrintableString;
use entity51_wire::Entity51Wire;
pub(crate) mod named_fields;
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
pub(crate) mod group_member;
use group_member::GroupMemberTarget;
pub(crate) mod group_record;
use crate::deltas::group::{GroupReferenceStatus, GroupSelector};
use group_record::GroupOrigin;

use super::substrate::{ParsedStreams, StreamView};

use std::collections::{BTreeMap, BTreeSet};

/// One complete Parasolid GROUP record with its source and owning-partition scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "group_record::GroupWire", into = "group_record::GroupWire")]
pub struct ParasolidGroupRecord {
    /// Globally unique source-record identity.
    pub id: String,
    /// Exact source stream and its partition namespace.
    pub origin: GroupOrigin,
    /// Stream-local XMT identity.
    pub xmt: u32,
    /// Partition-local kernel node identity.
    pub node_id: u32,
    /// Ordered GROUP references without their framing status bytes.
    pub references: [u32; 5],
    /// Selector between the four leading references and the linked reference.
    pub selector: GroupSelector,
    /// Status byte following the linked reference.
    pub linked_reference_status: GroupReferenceStatus,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// GROUP tag offset in the inflated source stream.
    pub inflated_offset: u64,
}

/// One topology member in a fully closed current Parasolid GROUP chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "group_member::MemberWire",
    into = "group_member::MemberWire"
)]
pub struct ParasolidGroupMember {
    /// Globally unique membership identity.
    pub id: String,
    /// Partition whose local XMT and node namespaces own the chain.
    pub partition_stream_ordinal: u32,
    /// Current GROUP record XMT identity.
    pub group_xmt: u32,
    /// Current GROUP kernel node identity.
    pub group_node_id: u32,
    /// Zero-based member order from the list head to tail.
    pub ordinal: u32,
    /// `TYPE_91` list-record XMT identity.
    pub list_record_xmt: u32,
    /// Member record XMT identity.
    pub member_xmt: u32,
    /// Member family with its required node and optional current identity.
    pub target: GroupMemberTarget,
}

/// Retain GROUP records from partition streams and raw deltas overlays.
///
/// Deltas records use the partition pairing already selected for topology
/// reconstruction. A record in an unpaired deltas stream remains exact native
/// evidence but has no partition-local namespace assignment.
pub(crate) fn parasolid_group_records(
    streams: &[Stream],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
    deltas_records: &[ParasolidDeltasRecord],
) -> Vec<ParasolidGroupRecord> {
    let paired_partition = delta_pairs
        .iter()
        .flat_map(|(partition, deltas)| {
            deltas
                .iter()
                .filter_map(move |delta| Some((*delta, u32::try_from(*partition).ok()?)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut groups = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if stream.kind() != crate::parasolid::StreamKind::Partition {
            continue;
        }
        let Ok(stream_ordinal_u32) = u32::try_from(stream_ordinal) else {
            continue;
        };
        for record in crate::deltas::census::walk(&stream.inflated)
            .into_events()
            .records
        {
            let crate::deltas::record_family::RecordFamily::Group {
                node_id,
                selector,
                linked_reference_status,
                references,
            } = record.family
            else {
                continue;
            };
            groups.push(ParasolidGroupRecord {
                id: format!(
                    "nx:s{stream_ordinal}:parasolid-group#{}-{}",
                    record.offset, record.xmt
                ),
                origin: GroupOrigin::Partition {
                    stream_ordinal: stream_ordinal_u32,
                },
                xmt: record.xmt,
                node_id,
                references,
                selector,
                linked_reference_status,
                byte_len: (record.end - record.offset) as u64,
                inflated_offset: record.offset as u64,
            });
        }
    }
    for record in deltas_records {
        let crate::deltas::record_family::RecordFamily::Group {
            node_id,
            selector,
            linked_reference_status,
            references,
        } = &record.family
        else {
            continue;
        };
        groups.push(ParasolidGroupRecord {
            id: record.id.replacen("deltas-record", "parasolid-group", 1),
            origin: GroupOrigin::Deltas {
                stream_ordinal: record.stream_ordinal,
                partition_stream_ordinal: usize::try_from(record.stream_ordinal)
                    .ok()
                    .and_then(|delta| paired_partition.get(&delta).copied()),
            },
            xmt: record.xmt,
            node_id: *node_id,
            references: *references,
            selector: *selector,
            linked_reference_status: *linked_reference_status,
            byte_len: record.byte_len,
            inflated_offset: record.inflated_offset,
        });
    }
    groups.sort_by_key(|group| (group.origin.stream_ordinal(), group.inflated_offset));
    groups
}

fn group_members_from_records(
    partition_stream_ordinal: u32,
    records: &[crate::deltas::Record],
) -> Vec<ParasolidGroupMember> {
    let mut records_by_xmt = BTreeMap::<u32, Vec<&crate::deltas::Record>>::new();
    for record in records {
        records_by_xmt.entry(record.xmt).or_default().push(record);
    }
    let unique_record = |xmt| match records_by_xmt.get(&xmt).map(Vec::as_slice) {
        Some([record]) => Some(*record),
        _ => None,
    };
    let mut groups_by_node = BTreeMap::<u32, Vec<(u32, u32)>>::new();
    for record in records {
        if let crate::deltas::record_family::RecordFamily::Group {
            node_id,
            references,
            ..
        } = &record.family
        {
            groups_by_node
                .entry(*node_id)
                .or_default()
                .push((record.xmt, references[4]));
        }
    }
    let mut members = Vec::new();
    for (&group_node_id, groups) in &groups_by_node {
        let &[(group_xmt, tail)] = groups.as_slice() else {
            continue;
        };
        let mut reverse_chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = tail;
        let mut expected_next = 1;
        let mut complete = true;
        while current != 1 {
            if !seen.insert(current) {
                complete = false;
                break;
            }
            let Some(list_record) = unique_record(current) else {
                complete = false;
                break;
            };
            let crate::deltas::record_family::RecordFamily::Type91 { references } =
                &list_record.family
            else {
                complete = false;
                break;
            };
            if references[0] != group_xmt || references[5] != expected_next {
                complete = false;
                break;
            }
            let member_xmt = references[1];
            let Some(member_record) = unique_record(member_xmt) else {
                complete = false;
                break;
            };
            let Some(target) = GroupMemberTarget::from_record(&member_record.family) else {
                complete = false;
                break;
            };
            reverse_chain.push((current, member_xmt, target));
            expected_next = current;
            current = references[4];
        }
        if !complete || reverse_chain.is_empty() {
            continue;
        }
        reverse_chain.reverse();
        members.extend(reverse_chain.into_iter().enumerate().filter_map(
            |(ordinal, (list_record_xmt, member_xmt, target))| {
                Some(ParasolidGroupMember {
                    id: format!(
                        "nx:s{partition_stream_ordinal}:parasolid-group-member#{group_node_id}-{group_xmt}-{ordinal}"
                    ),
                    partition_stream_ordinal,
                    group_xmt,
                    group_node_id,
                    ordinal: u32::try_from(ordinal).ok()?,
                    list_record_xmt,
                    member_xmt,
                    target,
                })
            },
        ));
    }
    members
}

fn apply_group_state_events(records: &mut BTreeMap<u32, crate::deltas::Record>, bytes: &[u8]) {
    enum Event {
        Record(crate::deltas::Record),
        Tombstone(u32),
    }
    let census = crate::deltas::census::walk(bytes).into_events();
    let mut events = census
        .records
        .into_iter()
        .map(|record| (record.offset, Event::Record(record)))
        .chain(
            census
                .tombstones
                .into_iter()
                .map(|tombstone| (tombstone.offset, Event::Tombstone(tombstone.xmt))),
        )
        .collect::<Vec<_>>();
    events.sort_by_key(|(offset, _)| *offset);
    for (_, event) in events {
        match event {
            Event::Record(record) => {
                records.insert(record.xmt, record);
            }
            Event::Tombstone(xmt) => {
                records.remove(&xmt);
            }
        }
    }
}

/// Resolve current GROUP membership from partition and ordered deltas events.
pub(crate) fn parasolid_group_members(
    streams: &[Stream],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
    parsed: &ParsedStreams<'_>,
) -> Vec<ParasolidGroupMember> {
    let mut members = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind() == crate::parasolid::StreamKind::Partition)
        .filter_map(|(stream_ordinal, stream)| {
            let stream_ordinal_u32 = u32::try_from(stream_ordinal).ok()?;
            let mut current = BTreeMap::new();
            apply_group_state_events(&mut current, &stream.inflated);
            for delta in delta_pairs.get(&stream_ordinal).into_iter().flatten() {
                apply_group_state_events(&mut current, &streams.get(*delta)?.inflated);
            }
            Some((
                stream_ordinal_u32,
                current.into_values().collect::<Vec<_>>(),
            ))
        })
        .flat_map(|(stream_ordinal, records)| group_members_from_records(stream_ordinal, &records))
        .collect::<Vec<_>>();
    for member in &mut members {
        let Ok(partition) = usize::try_from(member.partition_stream_ordinal) else {
            continue;
        };
        let graph = parsed.stream(partition).view_for_geometry().graph.as_ref();
        member.target = member.target.resolve(graph, member.member_xmt);
    }
    members
}

/// One completely bounded record in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "ParasolidDeltasRecordWire",
    into = "ParasolidDeltasRecordWire"
)]
pub struct ParasolidDeltasRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Semantic family, including POINT position and GROUP controls.
    pub family: crate::deltas::record_family::RecordFamily,
    /// Stream-local XMT identity.
    pub xmt: u32,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_selector: Option<GroupSelector>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_linked_reference_status: Option<GroupReferenceStatus>,
    position: Option<[f64; 3]>,
    byte_len: u64,
    inflated_offset: u64,
}

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
    use super::ParasolidDeltasRecord;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "ParasolidDeltasTombstoneWire",
    into = "ParasolidDeltasTombstoneWire"
)]
pub struct ParasolidDeltasTombstone {
    /// Globally unique event identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Numeric Parasolid node type.
    pub kind: crate::deltas::record_kind::RecordKind,
    /// Stream-local deleted XMT identity.
    pub xmt: u32,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
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

/// BODY revision envelope in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "body_revision_wire::RevisionWire",
    into = "body_revision_wire::RevisionWire"
)]
pub struct ParasolidDeltasBodyRevision {
    /// Globally unique revision identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local BODY XMT identity.
    pub xmt: NonNullXmt,
    /// Monotonic kernel revision identity.
    pub node_id: u32,
    /// Eight ordered BODY references.
    pub references: [u32; 8],
    /// Prefix and state-tail lengths with a representable total.
    pub lengths: RevisionLengths,
    /// SHA-256 of the exact bounded state-tail bytes.
    pub state_tail_sha256: crate::native::hex::Sha256Hex,
    /// BODY tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Parasolid transmit header at the start of a deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TransmitHeaderWire", into = "TransmitHeaderWire")]
pub struct ParasolidDeltasTransmitHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    pub state: TransmitState,
    /// Exact header byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact header bytes.
    pub sha256: crate::native::hex::Sha256Hex,
}

/// Null references at the boundary of a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "tail_wire::NullTailWire", into = "tail_wire::NullTailWire")]
pub struct ParasolidDeltasTerminalNullReferences {
    /// Globally unique trailer identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Complete two- or four-reference trailer form.
    pub form: NullTailForm,
    /// First trailer byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Count-selected numeric lane following one deltas `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "tail_wire::NumericTailWire",
    into = "tail_wire::NumericTailWire"
)]
pub struct ParasolidDeltasTermUseNumericTail {
    /// Globally unique numeric-tail identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// XMT identity of the owning `term_use` record.
    pub term_use_xmt: u32,
    /// Complete finite numeric tail for its endpoint count.
    pub values: NumericTailValues,
    /// First numeric byte following the complete `term_use` record.
    pub inflated_offset: u64,
}

/// Maximal deltas gap composed entirely of typed stream-local references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasTaggedReferenceLane {
    /// Globally unique reference-lane identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered `(Parasolid record kind, XMT identity)` references.
    pub references: TaggedReferences,
    /// Exact reference-lane byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact reference-lane bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First byte of the first tagged reference.
    pub inflated_offset: u64,
}

/// Framed reference/type map in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceTypeMap {
    /// Globally unique map identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered `(XMT identity, Parasolid type code)` entries.
    pub entries: MapEntries,
    /// Type code of the optional terminal map target.
    #[serde(default, deserialize_with = "deserialize_map_target_kind")]
    pub target_kind: Option<std::num::NonZeroU16>,
    /// Exact map byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact map bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First map byte offset in the inflated stream.
    pub inflated_offset: u64,
}

fn deserialize_map_target_kind<'de, D>(
    deserializer: D,
) -> Result<Option<std::num::NonZeroU16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u16>::deserialize(deserializer)?
        .map(|kind| {
            std::num::NonZeroU16::new(kind).ok_or_else(|| {
                serde::de::Error::custom("target_kind: must be nonzero when present")
            })
        })
        .transpose()
}

/// Reference-state packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceStatePacket {
    /// Globally unique packet identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Ordered packet frames.
    pub frames: StateFrames,
    /// Whether the packet ends with `ref(1)[3], u32(1)`.
    pub terminal: bool,
    /// Exact packet byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First packet byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Schema reference preamble in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasSchemaReferencePreamble {
    /// Globally unique preamble identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    #[serde(flatten)]
    pub state: PreambleState,
    /// Exact preamble byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact preamble bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First preamble byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Reference-marker packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasReferenceMarkerPacket {
    /// Globally unique packet identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Non-null stream-local XMT reference.
    pub reference: NonNullXmt,
    /// Serialized marker byte.
    pub marker: ReferenceMarker,
    /// Exact packet byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First packet byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Single-byte type-150 state packet in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidDeltasType150StatePacket {
    /// Globally unique packet identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Validated references, marker, and finite state values.
    #[serde(flatten)]
    pub state: Type150State,
    /// Exact packet byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact packet bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First packet byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Inline schema declaration in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidDeltasInlineSchemaDeclaration {
    /// Globally unique declaration identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Schema-specific declaration body.
    #[serde(flatten)]
    pub fields: InlineSchemaFields,
    /// Exact declaration byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact declaration bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First declaration byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Schema-bound type-12 `BODY` instance state in a Parasolid deltas stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasInlineBodyState {
    /// Globally unique state identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Serialized state form.
    pub fields: InlineBodyStateFields,
    /// Exact state byte length.
    pub byte_len: u64,
    /// SHA-256 of the exact state bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First state byte offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Maximal inflated-stream span outside every admitted deltas event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidDeltasResidualSpan {
    /// Globally unique residual-span identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Exact residual byte length.
    pub byte_len: u64,
    /// SHA-256 of the residual bytes.
    pub sha256: crate::native::hex::Sha256Hex,
    /// First residual byte offset in the inflated stream.
    pub inflated_offset: u64,
}

pub(crate) struct ParasolidDeltasEvents {
    pub(crate) transmit_headers: Vec<ParasolidDeltasTransmitHeader>,
    pub(crate) terminal_null_references: Vec<ParasolidDeltasTerminalNullReferences>,
    pub(crate) records: Vec<ParasolidDeltasRecord>,
    pub(crate) tombstones: Vec<ParasolidDeltasTombstone>,
    pub(crate) body_revisions: Vec<ParasolidDeltasBodyRevision>,
    pub(crate) term_use_numeric_tails: Vec<ParasolidDeltasTermUseNumericTail>,
    pub(crate) tagged_reference_lanes: Vec<ParasolidDeltasTaggedReferenceLane>,
    pub(crate) reference_type_maps: Vec<ParasolidDeltasReferenceTypeMap>,
    pub(crate) reference_state_packets: Vec<ParasolidDeltasReferenceStatePacket>,
    pub(crate) schema_reference_preambles: Vec<ParasolidDeltasSchemaReferencePreamble>,
    pub(crate) reference_marker_packets: Vec<ParasolidDeltasReferenceMarkerPacket>,
    pub(crate) type_150_state_packets: Vec<ParasolidDeltasType150StatePacket>,
    pub(crate) inline_schema_declarations: Vec<ParasolidDeltasInlineSchemaDeclaration>,
    pub(crate) inline_body_states: Vec<ParasolidDeltasInlineBodyState>,
    pub(crate) residual_spans: Vec<ParasolidDeltasResidualSpan>,
}

/// Retain every completely bounded event in every Parasolid deltas stream.
#[cfg(test)]
pub(crate) fn parasolid_deltas_events(streams: &[Stream]) -> ParasolidDeltasEvents {
    let delta_censuses = streams
        .iter()
        .map(|stream| {
            (stream.kind() == crate::parasolid::StreamKind::Deltas)
                .then(|| crate::deltas::census::walk(&stream.inflated))
        })
        .collect();
    parasolid_deltas_events_with_censuses(streams, delta_censuses)
}

/// Retain deltas events from censuses produced by the shared decode substrate.
///
/// The function consumes the census vector after semantic construction has
/// finished, so the large record walk is performed once and its owned records
/// are moved directly into native output.
pub(crate) fn parasolid_deltas_events_with_censuses(
    streams: &[Stream],
    mut delta_censuses: Vec<Option<Census>>,
) -> ParasolidDeltasEvents {
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
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if stream.kind() != crate::parasolid::StreamKind::Deltas {
            continue;
        }
        let census = delta_censuses
            .get_mut(stream_ordinal)
            .and_then(Option::take)
            .unwrap_or_else(|| crate::deltas::census::walk(&stream.inflated));
        let mut residual_start = 0;
        for (covered_start, covered_end) in census.covered_spans() {
            if residual_start < covered_start {
                push_deltas_residual_span(
                    &mut events.residual_spans,
                    stream_ordinal,
                    &stream.inflated,
                    residual_start,
                    covered_start,
                );
            }
            residual_start = residual_start.max(covered_end);
        }
        if residual_start < stream.inflated.len() {
            push_deltas_residual_span(
                &mut events.residual_spans,
                stream_ordinal,
                &stream.inflated,
                residual_start,
                stream.inflated.len(),
            );
        }
        let census = census.into_events();
        if let Some(header) = census.transmit_header {
            let bytes = &stream.inflated[..header.end];
            events.transmit_headers.push(ParasolidDeltasTransmitHeader {
                id: format!("nx:s{stream_ordinal}:deltas-transmit-header#0"),
                stream_ordinal: stream_ordinal as u32,
                state: header.state,
                byte_len: bytes.len() as u64,
                sha256: crate::native::hex::Sha256Hex::digest(bytes),
            });
        }
        if let Some(trailer) = census.terminal_null_references {
            events
                .terminal_null_references
                .push(ParasolidDeltasTerminalNullReferences {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-terminal-null-references#{}",
                        trailer.offset()
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    form: trailer.form(),
                    inflated_offset: trailer.offset() as u64,
                });
        }
        for record in census.records {
            events.records.push(ParasolidDeltasRecord {
                id: format!(
                    "nx:s{stream_ordinal}:deltas-record#{}-{}",
                    record.offset, record.xmt
                ),
                stream_ordinal: stream_ordinal as u32,
                family: record.family,
                xmt: record.xmt,
                byte_len: (record.end - record.offset) as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for tombstone in census.tombstones {
            events.tombstones.push(ParasolidDeltasTombstone {
                id: format!(
                    "nx:s{stream_ordinal}:deltas-tombstone#{}-{}",
                    tombstone.offset, tombstone.xmt
                ),
                stream_ordinal: stream_ordinal as u32,
                kind: tombstone.kind,
                xmt: tombstone.xmt,
                inflated_offset: tombstone.offset as u64,
            });
        }
        for revision in census.body_revisions {
            let state_tail = &stream.inflated[revision.prefix_end..revision.end];
            events.body_revisions.push(ParasolidDeltasBodyRevision {
                id: format!(
                    "nx:s{stream_ordinal}:deltas-body-revision#{}-{}",
                    revision.offset, revision.node_id
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: revision.xmt,
                node_id: revision.node_id,
                references: revision.references,
                lengths: RevisionLengths::from_slices(
                    &stream.inflated[revision.offset..revision.prefix_end],
                    state_tail,
                ),
                state_tail_sha256: crate::native::hex::Sha256Hex::digest(state_tail),
                inflated_offset: revision.offset as u64,
            });
        }
        for tail in census.term_use_numeric_tails {
            events
                .term_use_numeric_tails
                .push(ParasolidDeltasTermUseNumericTail {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-term-use-tail#{}-{}",
                        tail.offset(),
                        tail.term_use_xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    term_use_xmt: tail.term_use_xmt,
                    inflated_offset: tail.offset() as u64,
                    values: tail.into_values(),
                });
        }
        for lane in census.tagged_reference_lanes {
            let bytes = &stream.inflated[lane.offset..lane.end];
            events
                .tagged_reference_lanes
                .push(ParasolidDeltasTaggedReferenceLane {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-tagged-reference-lane#{}",
                        lane.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    references: lane.references,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: lane.offset as u64,
                });
        }
        for map in census.reference_type_maps {
            let bytes = &stream.inflated[map.offset..map.end];
            events
                .reference_type_maps
                .push(ParasolidDeltasReferenceTypeMap {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-reference-type-map#{}",
                        map.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    entries: map.entries,
                    target_kind: map.target_kind,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: map.offset as u64,
                });
        }
        for packet in census.reference_state_packets {
            let bytes = &stream.inflated[packet.offset..packet.end];
            events
                .reference_state_packets
                .push(ParasolidDeltasReferenceStatePacket {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-reference-state#{}",
                        packet.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    frames: packet.frames,
                    terminal: packet.terminal,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: packet.offset as u64,
                });
        }
        for preamble in census.schema_reference_preambles {
            let bytes = &stream.inflated[preamble.offset..preamble.end];
            events
                .schema_reference_preambles
                .push(ParasolidDeltasSchemaReferencePreamble {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-schema-reference-preamble#{}",
                        preamble.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    state: preamble.state,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: preamble.offset as u64,
                });
        }
        for packet in census.reference_marker_packets {
            let bytes = &stream.inflated[packet.offset..packet.end];
            events
                .reference_marker_packets
                .push(ParasolidDeltasReferenceMarkerPacket {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-reference-marker#{}",
                        packet.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    reference: packet.reference,
                    marker: packet.marker,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: packet.offset as u64,
                });
        }
        for packet in census.type_150_state_packets {
            let bytes = &stream.inflated[packet.offset..packet.end];
            events
                .type_150_state_packets
                .push(ParasolidDeltasType150StatePacket {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-type-150-state#{}",
                        packet.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    state: packet.state,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: packet.offset as u64,
                });
        }
        for declaration in census.inline_schema_declarations {
            let bytes = &stream.inflated[declaration.offset..declaration.end];
            events
                .inline_schema_declarations
                .push(ParasolidDeltasInlineSchemaDeclaration {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-inline-schema#{}",
                        declaration.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    fields: declaration.fields,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: declaration.offset as u64,
                });
        }
        for state in census.inline_body_states {
            let bytes = &stream.inflated[state.offset..state.end];
            events
                .inline_body_states
                .push(ParasolidDeltasInlineBodyState {
                    id: format!(
                        "nx:s{stream_ordinal}:deltas-inline-body-state#{}",
                        state.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    fields: state.fields,
                    byte_len: bytes.len() as u64,
                    sha256: crate::native::hex::Sha256Hex::digest(bytes),
                    inflated_offset: state.offset as u64,
                });
        }
    }
    events
        .transmit_headers
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .terminal_null_references
        .sort_by(|left, right| left.id.cmp(&right.id));
    events.records.sort_by(|left, right| left.id.cmp(&right.id));
    events
        .tombstones
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .body_revisions
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .term_use_numeric_tails
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .tagged_reference_lanes
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .reference_type_maps
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .reference_state_packets
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .schema_reference_preambles
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .reference_marker_packets
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .type_150_state_packets
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .inline_schema_declarations
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .inline_body_states
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
        .residual_spans
        .sort_by(|left, right| left.id.cmp(&right.id));
    events
}

fn push_deltas_residual_span(
    residual_spans: &mut Vec<ParasolidDeltasResidualSpan>,
    stream_ordinal: usize,
    bytes: &[u8],
    start: usize,
    end: usize,
) {
    let residual = &bytes[start..end];
    residual_spans.push(ParasolidDeltasResidualSpan {
        id: format!("nx:s{stream_ordinal}:deltas-residual#{start}"),
        stream_ordinal: stream_ordinal as u32,
        byte_len: residual.len() as u64,
        sha256: crate::native::hex::Sha256Hex::digest(residual),
        inflated_offset: start as u64,
    });
}

/// Shared skeleton for Parasolid record families read from the cached per-stream
/// record view. It owns the stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}`
/// identity, and the sort by identity; each family supplies only its cached row
/// slice and its record constructor.
pub(crate) trait ParasolidStreamRecords {
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

/// Run the cached-view record skeleton for one family: map every cached row of
/// every stream to a record, then sort by identity. Non-Parasolid streams hold
/// empty views, so no per-stream guard is needed.
pub(crate) fn per_parasolid_stream<P: ParasolidStreamRecords>(
    parsed: &ParsedStreams,
) -> Vec<P::Record> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        for row in P::rows(stream.view_for_records()) {
            let id = format!("nx:s{stream_ordinal}:{}#{}", P::ID_STEM, P::xmt(row));
            records.push(P::record(id, stream_ordinal as u32, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    records
}

/// Shared skeleton for Parasolid record families scanned fresh from each
/// Parasolid stream's inflated bytes. It owns the `is_parasolid()` guard, the
/// stream loop, the `nx:s{ordinal}:{ID_STEM}#{xmt}` identity, and the sort; each
/// family supplies only its scanner and its record constructor.
pub(crate) trait ParasolidScanRecords {
    /// Scanned row type produced from the inflated stream bytes.
    type Row;
    /// Emitted native record type.
    type Record;
    /// Identity stem between the `nx:s{ordinal}:` prefix and the `#{xmt}` suffix.
    const ID_STEM: &'static str;
    /// Scan one inflated Parasolid stream into its rows.
    fn scan(bytes: &[u8]) -> Vec<Self::Row>;
    /// Cross-reference index carried into the record identity.
    fn xmt(row: &Self::Row) -> u32;
    /// Build one record from its identity, stream ordinal, and scanned row.
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record;
    /// The identity of a built record, used as the sort key.
    fn id(record: &Self::Record) -> &str;
}

/// Run the fresh-scan record skeleton for one family: scan every Parasolid
/// stream, map each scanned row to a record, then sort by identity.
pub(crate) fn per_parasolid_scan<P: ParasolidScanRecords>(streams: &[Stream]) -> Vec<P::Record> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind().is_parasolid() {
            continue;
        }
        for row in P::scan(&stream.inflated) {
            let id = format!("nx:s{stream_ordinal}:{}#{}", P::ID_STEM, P::xmt(&row));
            records.push(P::record(id, stream_ordinal as u32, row));
        }
    }
    records.sort_by(|left, right| P::id(left).cmp(P::id(right)));
    records
}

/// Complete typed source record for one Parasolid offset surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidOffsetSurfaceRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the offset surface.
    pub xmt: u32,
    /// Serialized `V`, `I`, or `U` discriminator.
    pub discriminator: crate::topology::OffsetSurfaceDiscriminator,
    /// Serialized true-offset flag.
    pub true_offset: bool,
    /// Checked support reference and signed model distance.
    #[serde(flatten)]
    pub state: OffsetSurfaceState,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid offset surfaces.
pub(crate) fn parasolid_offset_surface_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidOffsetSurfaceRecord> {
    per_parasolid_stream::<ParasolidOffsetSurfaceRecord>(parsed)
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
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid trimmed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidTrimmedCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the trimmed curve.
    pub xmt: u32,
    #[serde(flatten)]
    pub state: crate::topology::trimmed_curve_state::TrimmedCurveState,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid trimmed curves.
pub(crate) fn parasolid_trimmed_curve_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidTrimmedCurveRecord> {
    per_parasolid_stream::<ParasolidTrimmedCurveRecord>(parsed)
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
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidSurfaceCurveRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the surface curve.
    pub xmt: u32,
    #[serde(flatten)]
    pub state: crate::topology::surface_curve_state::SurfaceCurveState,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid surface curves.
pub(crate) fn parasolid_surface_curve_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidSurfaceCurveRecord> {
    per_parasolid_stream::<ParasolidSurfaceCurveRecord>(parsed)
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
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid blend-bound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidBlendBoundRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    #[serde(flatten)]
    pub state: crate::intersection::blend_bound_state::BlendBoundState,
    /// Serialized partition/deltas and direct/escaped framing.
    pub framing: crate::intersection::BlendBoundFraming,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid blend-bound bridges.
pub fn parasolid_blend_bound_records(streams: &[Stream]) -> Vec<ParasolidBlendBoundRecord> {
    per_parasolid_scan::<ParasolidBlendBoundRecord>(streams)
}

impl ParasolidScanRecords for ParasolidBlendBoundRecord {
    type Row = crate::intersection::BlendBound;
    type Record = ParasolidBlendBoundRecord;
    const ID_STEM: &'static str = "blend-bound-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::blend_bounds(bytes)
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
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid `term_use` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "ParasolidTermUseRecordWire",
    into = "ParasolidTermUseRecordWire"
)]
pub struct ParasolidTermUseRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    pub xmt: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    pub form: crate::intersection::TermUseForm,
    /// Endpoint position in millimetres.
    pub point: FinitePoint,
    /// Serialized record framing.
    pub framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ParasolidTermUseRecordWire {
    /// Globally unique record identity.
    id: String,
    /// Zero-based source stream ordinal.
    stream_ordinal: u32,
    /// Cross-reference index of the endpoint.
    xmt: u32,
    /// Serialized leading count.
    count: u32,
    /// Two-byte endpoint-form discriminator as printable ASCII.
    form: crate::intersection::TermUseForm,
    /// Endpoint position in millimetres.
    point: FinitePoint,
    /// Serialized record framing.
    framing: crate::intersection::TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    inflated_offset: u64,
}

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
    fn finite_endpoint_retains_the_native_point_array_and_open_identity() {
        let json = r#"{"id":"term","stream_ordinal":0,"xmt":0,"count":2,"form":"TF","point":[0.0,-0.0,1.0],"framing":"direct","inflated_offset":10}"#;
        let record: ParasolidTermUseRecord = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
    }
}

/// Decode complete typed source records for Parasolid `term_use` endpoints.
pub fn parasolid_term_use_records(streams: &[Stream]) -> Vec<ParasolidTermUseRecord> {
    per_parasolid_scan::<ParasolidTermUseRecord>(streams)
}

impl ParasolidScanRecords for ParasolidTermUseRecord {
    type Row = crate::intersection::TermUse;
    type Record = ParasolidTermUseRecord;
    const ID_STEM: &'static str = "term-use-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::term_use_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidTermUseRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            form: row.form,
            point: row.point,
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one Parasolid support-UV values array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "support_uv_wire::SupportUvWire",
    into = "support_uv_wire::SupportUvWire"
)]
pub struct ParasolidSupportUvRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the values array.
    pub xmt: u32,
    /// Exact finite packed support tuples.
    pub values: crate::intersection::support_uv_values::SupportUvValues,
    /// Serialized record framing.
    pub framing: crate::intersection::SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for Parasolid support-UV arrays.
pub fn parasolid_support_uv_records(streams: &[Stream]) -> Vec<ParasolidSupportUvRecord> {
    per_parasolid_scan::<ParasolidSupportUvRecord>(streams)
}

impl ParasolidScanRecords for ParasolidSupportUvRecord {
    type Row = crate::intersection::SupportUvRecord;
    type Record = ParasolidSupportUvRecord;
    const ID_STEM: &'static str = "support-uv-record";
    fn scan(bytes: &[u8]) -> Vec<Self::Row> {
        crate::intersection::support_uv_records(bytes)
    }
    fn xmt(row: &Self::Row) -> u32 {
        row.xmt
    }
    fn record(id: String, stream_ordinal: u32, row: Self::Row) -> Self::Record {
        ParasolidSupportUvRecord {
            id,
            stream_ordinal,
            xmt: row.xmt,
            values: row.values,
            framing: row.framing,
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed source record for one physical Parasolid `CHART_s` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "chart_wire::ChartWire", into = "chart_wire::ChartWire")]
pub struct ParasolidChartRecord {
    /// Globally unique physical-record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the chart.
    pub xmt: u32,
    /// Checked chart preamble.
    pub preamble: crate::intersection::chart_samples::ChartPreamble,
    /// Points with exactly the fields admitted by their Hvec layout.
    pub data: crate::intersection::chart_samples::SourceChartData,
    /// Serialized record framing.
    pub framing: crate::intersection::ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode every complete physical Parasolid chart source record.
pub fn parasolid_chart_records(streams: &[Stream]) -> Vec<ParasolidChartRecord> {
    let mut records = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        let crate::parasolid::StreamBody::Parasolid { subtype, .. } = &stream.body else {
            continue;
        };
        let point_layout = subtype.chart_point_layout();
        for chart in crate::intersection::chart_source_records(&stream.inflated, point_layout) {
            records.push(ParasolidChartRecord {
                id: format!(
                    "nx:s{stream_ordinal}:chart-record#{}-{}",
                    chart.xmt, chart.pos
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: chart.xmt,
                preamble: chart.preamble,
                data: chart.data,
                framing: chart.framing,
                inflated_offset: chart.pos as u64,
            });
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records
}

/// Complete typed source record for one Parasolid surface-intersection curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidIntersectionRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Cross-reference index of the construction.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Six ordered support and witness references.
    pub construction_references: [u32; 6],
    /// Whether the record uses the single-byte delta-twin tag.
    pub delta_twin: bool,
    /// Record tag offset in the inflated stream.
    pub inflated_offset: u64,
}

/// Decode complete typed source records for retained intersection constructions.
pub(crate) fn parasolid_intersection_records(
    parsed: &ParsedStreams<'_>,
) -> Vec<ParasolidIntersectionRecord> {
    per_parasolid_stream::<ParasolidIntersectionRecord>(parsed)
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
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Complete typed type-56 rolling-ball blend-surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidBlendSurfaceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local `BLEND_SURF` identity.
    pub xmt: u32,
    /// Checked supports, offsets, and thumb weights.
    #[serde(flatten)]
    pub state: BlendSurfaceState,
    /// Offset of the type tag in the inflated stream.
    pub inflated_offset: u64,
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

// Serde's `skip_serializing_if` callback is required to receive `&T`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_legal_owner_flag_count(value: &u8) -> bool {
    *value == default_legal_owner_flag_count()
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "ParasolidAttributeDefinitionWire",
    into = "ParasolidAttributeDefinitionWire"
)]
pub struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub xmt: NonNullXmt,
    /// Optional stream-local next-definition target.
    pub next_definition_xmt: Option<XmtTarget>,
    /// Stream-local type-79 identifier identity.
    pub identifier_xmt: NonNullXmt,
    /// Offset of the resolved type-79 identifier in the inflated stream.
    pub identifier_inflated_offset: u64,
    /// Exact printable attribute class name.
    pub name: PrintableString<String>,
    /// Numeric attribute type identifier.
    pub type_id: NonZeroU32,
    /// Ordered actions for the eight logged event families.
    pub action_codes: [AttributeAction; 8],
    /// Optional stream-local field-name-list target.
    pub field_names_xmt: Option<XmtTarget>,
    /// Ordered legal-owner flags.
    pub legal_owner_flags: crate::parasolid::LegalOwnerFlags,
    /// One serialized code for every declared field.
    pub field_codes: Vec<AttributeField>,
    /// Offset of the declaration in the inflated stream.
    pub inflated_offset: u64,
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

impl From<ParasolidAttributeDefinition> for ParasolidAttributeDefinitionWire {
    fn from(value: ParasolidAttributeDefinition) -> Self {
        Self {
            legal_owner_flag_count: value.legal_owner_flags.as_slice().len() as u8,
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
pub struct ParasolidFieldNamesRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered stream-local character or Unicode value references.
    pub name_xmts: NameReferences,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Complete type-80 declaration-to-field-name-list relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "named_fields::FieldNamesWire",
    into = "named_fields::FieldNamesWire"
)]
pub struct ParasolidAttributeFieldNames {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-80 declaration.
    pub attribute_definition: String,
    /// Uniquely resolved type-99 field-name record.
    pub field_names_record: String,
    /// Ordered exact names paired with their resolved value records.
    pub fields: Vec<NamedField>,
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Parasolid topology record type.
    pub topology_type: TopologyAttributeKind,
    /// Stream-local topology-record identity.
    pub topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Entity51Wire", into = "Entity51Wire")]
pub struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Serialized sequence value.
    pub sequence: NonZeroU32,
    /// Stream-local type-80 attribute-definition identity.
    pub definition_xmt: u32,
    /// Five fixed leading stream-local references.
    pub leading_references: [u32; 5],
    /// Variable trailing stream-local references counted by `flags`.
    pub trailing_references: EntityReferences,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Exact nonempty printable value.
    pub value: PrintableString<String>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered big-endian unsigned values.
    pub values: CountedValues<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered finite big-endian binary64 values.
    pub values: CountedValues<f64>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Parasolid vector-shaped attribute-value family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidVectorValueKind {
    /// Type-85 point values.
    Points,
    /// Type-86 free-vector values.
    Vectors,
    /// Type-89 direction values.
    Directions,
}

/// Counted Parasolid type-85, type-86, or type-89 vector-shaped value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntityVectorRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Exact value family.
    pub kind: ParasolidVectorValueKind,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered finite xyz values.
    pub values: CountedValues<[f64; 3]>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-87 axis-value record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity57AxisRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered axes, each retaining its two serialized xyz vectors.
    pub values: CountedValues<[[f64; 3]; 2]>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-88 tag-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity58TagRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Ordered exact tag values.
    pub values: CountedValues<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-98 Unicode-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "ParasolidEntity62UnicodeRecordWire",
    into = "ParasolidEntity62UnicodeRecordWire"
)]
pub struct ParasolidEntity62UnicodeRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: NonNullXmt,
    /// Validated Unicode scalar string.
    pub value: UnicodeValue,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
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
pub(crate) struct ParasolidEntityValueRecords {
    pub(crate) integers: Vec<ParasolidEntity52IntegerRecord>,
    pub(crate) doubles: Vec<ParasolidEntity53DoubleRecord>,
    pub(crate) strings: Vec<ParasolidEntity54StringRecord>,
    pub(crate) vectors: Vec<ParasolidEntityVectorRecord>,
    pub(crate) axes: Vec<ParasolidEntity57AxisRecord>,
    pub(crate) tags: Vec<ParasolidEntity58TagRecord>,
    pub(crate) unicode: Vec<ParasolidEntity62UnicodeRecord>,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub position: FieldPosition,
    /// Stream-local referenced xmt.
    pub referenced_xmt: NonNullXmt,
    /// Numeric record family.
    pub kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub position: FieldPosition,
    /// Stream-local referenced xmt.
    pub referenced_xmt: NonNullXmt,
    /// Uniquely resolved type-84 string record.
    pub string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to one uniquely resolved structured value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StructuredUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    #[serde(rename = "reference_ordinal")]
    pub position: FieldPosition,
    /// Stream-local referenced xmt.
    pub referenced_xmt: NonNullXmt,
    /// Structured value-record family.
    pub kind: StructuredValueKind,
    /// Uniquely resolved structured value record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved registered class of one Parasolid type-81 attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "ParasolidAttributeClassUseWire")]
pub struct ParasolidAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Stream-local XMT of the matched type-80 definition.
    pub definition_xmt: NonNullXmt,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct ParasolidAttributeClassUseWire {
    id: String,
    stream_ordinal: u32,
    entity_51_record: String,
    definition_xmt: NonNullXmt,
    attribute_definition: String,
}

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
pub enum ParasolidAttributeFieldValueKind {
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
    pub(crate) fn field_code(self) -> AttributeField {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FieldUseWire", into = "FieldUseWire")]
pub struct ParasolidAttributeFieldUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Resolved class relation for the attribute instance.
    pub attribute_class_use: String,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
    /// Position in the field declaration and complete type-81 reference lane.
    pub position: FieldPosition,
    /// Resolved value-record family.
    pub value_kind: ParasolidAttributeFieldValueKind,
    /// Type-81-to-value relation carrying this field.
    pub value_use: String,
    /// Uniquely resolved value record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved class of one topology-owned Parasolid attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "ParasolidTopologyAttributeClassUseWire")]
pub struct ParasolidTopologyAttributeClassUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning topology-to-attribute relation.
    pub topology_attribute_reference: String,
    /// Topology-owned type-81 attribute-instance record.
    pub entity_51_record: String,
    /// Resolved class relation for the attribute instance.
    pub attribute_class_use: String,
    /// Stream-local XMT of the matched type-80 definition.
    pub definition_xmt: NonNullXmt,
    /// Uniquely matched attribute definition.
    pub attribute_definition: String,
    /// Zero-based source stream ordinal.
    pub stream_ordinal: u32,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct ParasolidTopologyAttributeClassUseWire {
    id: String,
    topology_attribute_reference: String,
    entity_51_record: String,
    attribute_class_use: String,
    definition_xmt: NonNullXmt,
    attribute_definition: String,
}

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
pub fn parasolid_attribute_definitions(streams: &[Stream]) -> Vec<ParasolidAttributeDefinition> {
    streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind().is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::attribute_definitions(&stream.inflated)
                .into_iter()
                .map(move |definition| ParasolidAttributeDefinition {
                    id: format!(
                        "nx:s{stream_ordinal}:attribute-definition#{}",
                        u32::from(definition.xmt)
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: definition.xmt,
                    next_definition_xmt: definition.next_definition_xmt,
                    identifier_xmt: definition.identifier_xmt,
                    identifier_inflated_offset: definition.identifier_offset as u64,
                    name: definition.name.into_owned(),
                    type_id: definition.type_id,
                    action_codes: definition.action_codes,
                    field_names_xmt: definition.field_names_xmt,
                    legal_owner_flags: definition.legal_owner_flags,
                    field_codes: definition.field_codes,
                    inflated_offset: definition.offset as u64,
                })
        })
        .collect()
}

/// Decode every counted type-99 attribute field-name record.
pub fn parasolid_field_names_records(streams: &[Stream]) -> Vec<ParasolidFieldNamesRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind().is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::field_names_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidFieldNamesRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:field-names#{}-{}",
                        u32::from(record.xmt),
                        record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    name_xmts: record.name_xmts,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Resolve complete type-80 field-name lists through type-99 and character records.
pub fn parasolid_attribute_field_names(
    definitions: &[ParasolidAttributeDefinition],
    field_names: &[ParasolidFieldNamesRecord],
    strings: &[ParasolidEntity54StringRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Vec<ParasolidAttributeFieldNames> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u32), Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_identity
            .entry((definition.stream_ordinal, u32::from(definition.xmt)))
            .or_default()
            .push(definition);
    }
    let mut lists = BTreeMap::<(u32, u32), Vec<&ParasolidFieldNamesRecord>>::new();
    for list in field_names {
        lists
            .entry((list.stream_ordinal, u32::from(list.xmt)))
            .or_default()
            .push(list);
    }
    let mut names_by_xmt = BTreeMap::<(u32, u32), Vec<(&str, &str)>>::new();
    for string in strings {
        names_by_xmt
            .entry((string.stream_ordinal, u32::from(string.xmt)))
            .or_default()
            .push((string.id.as_str(), string.value.as_str()));
    }
    for value in unicode {
        names_by_xmt
            .entry((value.stream_ordinal, u32::from(value.xmt)))
            .or_default()
            .push((value.id.as_str(), value.value.as_str()));
    }
    let mut relations = definitions_by_identity
        .values()
        .filter_map(|definitions| {
            let [definition] = definitions.as_slice() else {
                return None;
            };
            Some(*definition)
        })
        .filter_map(|definition| {
            let [list] = lists
                .get(&(
                    definition.stream_ordinal,
                    u32::from(definition.field_names_xmt?),
                ))?
                .as_slice()
            else {
                return None;
            };
            (list.name_xmts.as_slice().len() == definition.field_codes.len()).then_some(())?;
            let resolved = list
                .name_xmts
                .as_slice()
                .iter()
                .map(|xmt| {
                    let [name] = names_by_xmt
                        .get(&(definition.stream_ordinal, u32::from(*xmt)))?
                        .as_slice()
                    else {
                        return None;
                    };
                    Some(NamedField {
                        value_record: name.0.to_string(),
                        name: name.1.to_string(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(ParasolidAttributeFieldNames {
                id: format!(
                    "nx:s{}:attribute-field-names#{}",
                    definition.stream_ordinal,
                    u32::from(definition.xmt)
                ),
                stream_ordinal: definition.stream_ordinal,
                attribute_definition: definition.id.clone(),
                field_names_record: list.id.clone(),
                fields: resolved,
            })
        })
        .collect::<Vec<_>>();
    relations.sort_by(|first, second| first.id.cmp(&second.id));
    relations
}

/// Retain complete typed rolling-ball blend records from all Parasolid streams.
pub(crate) fn parasolid_blend_surface_records(
    parsed: &ParsedStreams,
) -> Vec<ParasolidBlendSurfaceRecord> {
    per_parasolid_stream::<ParasolidBlendSurfaceRecord>(parsed)
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
            inflated_offset: row.pos as u64,
        }
    }
    fn id(record: &Self::Record) -> &str {
        &record.id
    }
}

/// Retain every non-null topology-to-attribute-list reference.
pub(crate) fn parasolid_topology_attribute_list_references(
    parsed: &ParsedStreams,
    entity_records: &[ParasolidEntity51Record],
) -> Vec<ParasolidTopologyAttributeListReference> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for record in entity_records {
        records_by_identity
            .entry((record.stream_ordinal, u32::from(record.xmt)))
            .or_default()
            .push(record.id.as_str());
    }
    let mut references = Vec::new();
    for (stream_ordinal, stream) in parsed.iter() {
        let graph = &stream.view_for_records().graph;
        for topology_type in TopologyAttributeKind::ALL {
            for node in graph.of_kind(topology_type.node_kind()) {
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
                references.push(ParasolidTopologyAttributeListReference {
                    id: format!(
                        "nx:s{stream_ordinal}:topology-attribute-list-reference#{}-{}",
                        topology_type.code(),
                        node.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    topology_type,
                    topology_xmt: node.xmt,
                    attribute_list_xmt,
                    attribute_list_record: records_by_identity
                        .get(&(stream_ordinal as u32, attribute_list_xmt))
                        .and_then(|records| {
                            let [record] = records.as_slice() else {
                                return None;
                            };
                            Some((*record).to_string())
                        }),
                    inflated_offset: inflated_offset as u64,
                });
            }
        }
    }
    references
}

/// Decode every framed type-81 entity/attribute-list record.
pub fn parasolid_entity_51_records(streams: &[Stream]) -> Vec<ParasolidEntity51Record> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind().is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_51_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity51Record {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-51#{}-{}",
                        u32::from(record.xmt),
                        record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    sequence: record.sequence,
                    definition_xmt: record.definition_xmt,
                    leading_references: record.leading_references,
                    trailing_references: record.trailing_references,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode value records from their retained deltas or attribute owners.
pub(crate) fn parasolid_entity_value_records(
    streams: &[Stream],
    deltas_records: &[ParasolidDeltasRecord],
) -> ParasolidEntityValueRecords {
    let mut records = ParasolidEntityValueRecords {
        integers: Vec::new(),
        doubles: Vec::new(),
        strings: Vec::new(),
        vectors: Vec::new(),
        axes: Vec::new(),
        tags: Vec::new(),
        unicode: Vec::new(),
    };
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        let owned_offsets = match stream.kind() {
            StreamKind::Deltas => deltas_records
                .iter()
                .filter_map(|record| {
                    (record.stream_ordinal == stream_ordinal as u32
                        && matches!(
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
                        ))
                    .then(|| usize::try_from(record.inflated_offset).ok())
                    .flatten()
                })
                .collect::<Vec<_>>(),
            StreamKind::Partition | StreamKind::Plain => {
                crate::parasolid::referenced_value_record_offsets(&stream.inflated)
            }
            StreamKind::Preview => continue,
        };
        let values = crate::parasolid::value_records::entity_value_records_at(
            &stream.inflated,
            owned_offsets,
        );
        for record in values.integers {
            records.integers.push(ParasolidEntity52IntegerRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-52-integers#{}-{}",
                    u32::from(record.xmt),
                    record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.value,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.doubles {
            records.doubles.push(ParasolidEntity53DoubleRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-53-doubles#{}-{}",
                    u32::from(record.xmt),
                    record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.value,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.strings {
            records.strings.push(ParasolidEntity54StringRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-54-string#{}-{}",
                    u32::from(record.xmt),
                    record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                value: record.value.into_owned(),
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        let mut retain_vector = |kind, family: &str, xmt, offset, byte_len, values| {
            records.vectors.push(ParasolidEntityVectorRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-{family}#{}-{offset}",
                    u32::from(xmt)
                ),
                stream_ordinal: stream_ordinal as u32,
                kind,
                xmt,
                values,
                byte_len: byte_len as u64,
                inflated_offset: offset as u64,
            });
        };
        for record in values.points {
            retain_vector(
                ParasolidVectorValueKind::Points,
                "55-points",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            );
        }
        for record in values.vectors {
            retain_vector(
                ParasolidVectorValueKind::Vectors,
                "56-vectors",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            );
        }
        for record in values.directions {
            retain_vector(
                ParasolidVectorValueKind::Directions,
                "59-directions",
                record.xmt,
                record.offset,
                record.byte_len,
                record.value,
            );
        }
        for record in values.axes {
            records.axes.push(ParasolidEntity57AxisRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-57-axes#{}-{}",
                    u32::from(record.xmt),
                    record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.value,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.tags {
            records.tags.push(ParasolidEntity58TagRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-58-tags#{}-{}",
                    u32::from(record.xmt),
                    record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                values: record.value,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
        for record in values.unicode {
            records.unicode.push(ParasolidEntity62UnicodeRecord {
                id: format!(
                    "nx:s{stream_ordinal}:entity-62-unicode#{}-{}",
                    u32::from(record.xmt),
                    record.offset
                ),
                stream_ordinal: stream_ordinal as u32,
                xmt: record.xmt,
                value: record.value,
                byte_len: record.byte_len as u64,
                inflated_offset: record.offset as u64,
            });
        }
    }
    records
        .integers
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .doubles
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .strings
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .vectors
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .axes
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .tags
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
        .unicode
        .sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Join type-81 reference slots to unique same-stream numeric value records.
pub fn parasolid_entity_51_numeric_uses(
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Vec<ParasolidEntity51NumericUse> {
    let mut values =
        BTreeMap::<(u32, u32), Vec<(ParasolidEntity51NumericKind, NonNullXmt, &str)>>::new();
    for record in integers {
        values
            .entry((record.stream_ordinal, u32::from(record.xmt)))
            .or_default()
            .push((
                ParasolidEntity51NumericKind::UnsignedIntegers,
                record.xmt,
                &record.id,
            ));
    }
    for record in doubles {
        values
            .entry((record.stream_ordinal, u32::from(record.xmt)))
            .or_default()
            .push((
                ParasolidEntity51NumericKind::Doubles,
                record.xmt,
                &record.id,
            ));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some([(kind, target_xmt, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51NumericUse {
                id: format!(
                    "nx:s{}:entity-51-numeric-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal,
                    u32::from(entity.xmt),
                    entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                position,
                referenced_xmt: *target_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub fn parasolid_entity_51_string_uses(
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Vec<ParasolidEntity51StringUse> {
    let mut strings_by_identity =
        BTreeMap::<(u32, u32), Vec<&ParasolidEntity54StringRecord>>::new();
    for string in strings {
        strings_by_identity
            .entry((string.stream_ordinal, u32::from(string.xmt)))
            .or_default()
            .push(string);
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some([string]) = strings_by_identity
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StringUse {
                id: format!(
                    "nx:s{}:entity-51-string-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal,
                    u32::from(entity.xmt),
                    entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                position,
                referenced_xmt: string.xmt,
                string_record: string.id.clone(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream structured value records.
pub fn parasolid_entity_51_structured_uses(
    entities: &[ParasolidEntity51Record],
    vectors: &[ParasolidEntityVectorRecord],
    axes: &[ParasolidEntity57AxisRecord],
    tags: &[ParasolidEntity58TagRecord],
    unicode: &[ParasolidEntity62UnicodeRecord],
) -> Vec<ParasolidEntity51StructuredUse> {
    let mut values = BTreeMap::<(u32, u32), Vec<(StructuredValueKind, NonNullXmt, &str)>>::new();
    for record in vectors {
        let kind = match record.kind {
            ParasolidVectorValueKind::Points => StructuredValueKind::Points,
            ParasolidVectorValueKind::Vectors => StructuredValueKind::Vectors,
            ParasolidVectorValueKind::Directions => StructuredValueKind::Directions,
        };
        values
            .entry((record.stream_ordinal, u32::from(record.xmt)))
            .or_default()
            .push((kind, record.xmt, record.id.as_str()));
    }
    for (kind, stream_ordinal, xmt, id) in axes
        .iter()
        .map(|record| {
            (
                StructuredValueKind::Axes,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        })
        .chain(tags.iter().map(|record| {
            (
                StructuredValueKind::Tags,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        }))
        .chain(unicode.iter().map(|record| {
            (
                StructuredValueKind::Unicode,
                record.stream_ordinal,
                record.xmt,
                record.id.as_str(),
            )
        }))
    {
        values
            .entry((stream_ordinal, u32::from(xmt)))
            .or_default()
            .push((kind, xmt, id));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (position, &referenced_xmt) in entity.trailing_references.fields() {
            let reference_ordinal = position.reference_ordinal();
            let Some([(kind, target_xmt, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StructuredUse {
                id: format!(
                    "nx:s{}:entity-51-structured-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal,
                    u32::from(entity.xmt),
                    entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                position,
                referenced_xmt: *target_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve topology-owned attribute instances through their type-80 definition.
pub fn parasolid_topology_attribute_class_uses(
    topology_references: &[ParasolidTopologyAttributeListReference],
    entity_records: &[ParasolidEntity51Record],
    class_uses: &[ParasolidAttributeClassUse],
) -> Vec<ParasolidTopologyAttributeClassUse> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Vec<&ParasolidEntity51Record>>::new();
    for record in entity_records {
        records_by_identity
            .entry((record.stream_ordinal, u32::from(record.xmt)))
            .or_default()
            .push(record);
    }
    let mut records_by_owner = BTreeMap::<(u32, u32), Vec<&ParasolidEntity51Record>>::new();
    for record in entity_records {
        let owner_xmt = record.leading_references[0];
        if owner_xmt > 1 {
            records_by_owner
                .entry((record.stream_ordinal, owner_xmt))
                .or_default()
                .push(record);
        }
    }
    let mut class_uses_by_entity = BTreeMap::<&str, Vec<&ParasolidAttributeClassUse>>::new();
    for class_use in class_uses {
        class_uses_by_entity
            .entry(class_use.entity_51_record.as_str())
            .or_default()
            .push(class_use);
    }
    let mut uses = Vec::new();
    for reference in topology_references {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some([head]) = records_by_identity
            .get(&(reference.stream_ordinal, reference.attribute_list_xmt))
            .map(Vec::as_slice)
        else {
            continue;
        };
        if head.id != entity_id || head.leading_references[0] != reference.topology_xmt {
            continue;
        }

        let Some(members) =
            records_by_owner.get(&(reference.stream_ordinal, reference.topology_xmt))
        else {
            continue;
        };

        let base_id = format!(
            "nx:s{}:topology-attribute-class-use#{}-{}",
            reference.stream_ordinal,
            reference.topology_type.code(),
            reference.topology_xmt
        );
        let mut member_xmt_counts = BTreeMap::<u32, usize>::new();
        for member in members {
            *member_xmt_counts.entry(u32::from(member.xmt)).or_default() += 1;
        }
        for member in members {
            let Some([class_use]) = class_uses_by_entity
                .get(member.id.as_str())
                .map(Vec::as_slice)
            else {
                continue;
            };
            let id = if member.id == head.id {
                base_id.clone()
            } else if member_xmt_counts.get(&u32::from(member.xmt)) == Some(&1) {
                format!("{base_id}-{}", u32::from(member.xmt))
            } else {
                format!(
                    "{base_id}-{}-{}",
                    u32::from(member.xmt),
                    member.inflated_offset
                )
            };
            uses.push(ParasolidTopologyAttributeClassUse {
                stream_ordinal: reference.stream_ordinal,
                inflated_offset: member.inflated_offset,
                id,
                topology_attribute_reference: reference.id.clone(),
                entity_51_record: class_use.entity_51_record.clone(),
                attribute_class_use: class_use.id.clone(),
                definition_xmt: class_use.definition_xmt,
                attribute_definition: class_use.attribute_definition.clone(),
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Resolve every type-81 attribute instance through its type-80 definition reference.
pub fn parasolid_attribute_class_uses(
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Vec<ParasolidAttributeClassUse> {
    let mut definitions_by_identity =
        BTreeMap::<(u32, u32), Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_identity
            .entry((definition.stream_ordinal, u32::from(definition.xmt)))
            .or_default()
            .push(definition);
    }
    let mut uses = entities
        .iter()
        .filter_map(|entity| {
            let definition_xmt = entity.definition_xmt;
            let [definition] = definitions_by_identity
                .get(&(entity.stream_ordinal, definition_xmt))?
                .as_slice()
            else {
                return None;
            };
            Some(ParasolidAttributeClassUse {
                inflated_offset: entity.inflated_offset,
                id: format!(
                    "nx:s{}:attribute-class-use#{}-{}",
                    entity.stream_ordinal,
                    u32::from(entity.xmt),
                    entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                definition_xmt: definition.xmt,
                attribute_definition: definition.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Assign uniquely resolved attribute values to declared type-80 fields.
pub fn parasolid_attribute_field_uses(
    class_uses: &[ParasolidAttributeClassUse],
    definitions: &[ParasolidAttributeDefinition],
    numeric_uses: &[ParasolidEntity51NumericUse],
    string_uses: &[ParasolidEntity51StringUse],
    structured_uses: &[ParasolidEntity51StructuredUse],
) -> Vec<ParasolidAttributeFieldUse> {
    let mut classes = BTreeMap::<&str, Vec<&ParasolidAttributeClassUse>>::new();
    for class_use in class_uses {
        classes
            .entry(class_use.entity_51_record.as_str())
            .or_default()
            .push(class_use);
    }
    let mut definitions_by_id = BTreeMap::<&str, Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .push(definition);
    }
    let mut candidates = BTreeMap::<(&str, FieldPosition), Vec<_>>::new();
    for numeric_use in numeric_uses {
        let value_kind = match numeric_use.kind {
            ParasolidEntity51NumericKind::UnsignedIntegers => {
                ParasolidAttributeFieldValueKind::UnsignedIntegers
            }
            ParasolidEntity51NumericKind::Doubles => ParasolidAttributeFieldValueKind::Doubles,
        };
        candidates
            .entry((numeric_use.entity_51_record.as_str(), numeric_use.position))
            .or_default()
            .push((
                numeric_use.stream_ordinal,
                value_kind,
                numeric_use.id.as_str(),
                numeric_use.value_record.as_str(),
                numeric_use.inflated_offset,
            ));
    }
    for string_use in string_uses {
        candidates
            .entry((string_use.entity_51_record.as_str(), string_use.position))
            .or_default()
            .push((
                string_use.stream_ordinal,
                ParasolidAttributeFieldValueKind::String,
                string_use.id.as_str(),
                string_use.string_record.as_str(),
                string_use.inflated_offset,
            ));
    }
    for structured_use in structured_uses {
        candidates
            .entry((
                structured_use.entity_51_record.as_str(),
                structured_use.position,
            ))
            .or_default()
            .push((
                structured_use.stream_ordinal,
                structured_use.kind.into(),
                structured_use.id.as_str(),
                structured_use.value_record.as_str(),
                structured_use.inflated_offset,
            ));
    }
    let mut uses = candidates
        .into_iter()
        .filter_map(|((entity_51_record, position), candidates)| {
            let [(stream_ordinal, value_kind, value_use, value_record, inflated_offset)] =
                candidates.as_slice()
            else {
                return None;
            };
            let [class_use] = classes.get(entity_51_record)?.as_slice() else {
                return None;
            };
            if class_use.stream_ordinal != *stream_ordinal {
                return None;
            }
            let [definition] = definitions_by_id
                .get(class_use.attribute_definition.as_str())?
                .as_slice()
            else {
                return None;
            };
            let field_ordinal = position.field_ordinal();
            let field_code = *definition.field_codes.get(field_ordinal as usize)?;
            (field_code == value_kind.field_code()).then_some(())?;
            let (_, class_key) = class_use.id.rsplit_once('#')?;
            Some(ParasolidAttributeFieldUse {
                id: format!("nx:s{stream_ordinal}:attribute-field-use#{class_key}-{field_ordinal}"),
                stream_ordinal: *stream_ordinal,
                attribute_class_use: class_use.id.clone(),
                entity_51_record: entity_51_record.to_string(),
                attribute_definition: class_use.attribute_definition.clone(),
                position,
                value_kind: *value_kind,
                value_use: (*value_use).to_string(),
                value_record: (*value_record).to_string(),
                inflated_offset: *inflated_offset,
            })
        })
        .collect::<Vec<_>>();
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Whether a concrete topology-owned attribute field lacks its exact value relation.
pub fn parasolid_topology_attribute_fields_have_untransferred_values(
    definitions: &[ParasolidAttributeDefinition],
    entities: &[ParasolidEntity51Record],
    field_uses: &[ParasolidAttributeFieldUse],
    topology_class_uses: &[ParasolidTopologyAttributeClassUse],
) -> bool {
    let mut definitions_by_id = BTreeMap::<&str, Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .push(definition);
    }
    let mut fields_by_identity = BTreeMap::<(&str, u32), Vec<&ParasolidAttributeFieldUse>>::new();
    for field_use in field_uses {
        fields_by_identity
            .entry((
                field_use.entity_51_record.as_str(),
                field_use.position.field_ordinal(),
            ))
            .or_default()
            .push(field_use);
    }

    let mut entities_by_id = BTreeMap::<&str, Vec<&ParasolidEntity51Record>>::new();
    for entity in entities {
        entities_by_id
            .entry(entity.id.as_str())
            .or_default()
            .push(entity);
    }

    topology_class_uses.iter().any(|topology_class_use| {
        let entity_id = topology_class_use.entity_51_record.as_str();
        let Some([entity]) = entities_by_id.get(entity_id).map(Vec::as_slice) else {
            return true;
        };
        let Some([definition]) = definitions_by_id
            .get(topology_class_use.attribute_definition.as_str())
            .map(Vec::as_slice)
        else {
            return true;
        };
        definition
            .field_codes
            .iter()
            .enumerate()
            .any(|(field_ordinal, field_code)| {
                // Field code 0 is ignored. Pointer fields (code 9) are always
                // transmitted empty and therefore have no value relation.
                if matches!(
                    field_code,
                    AttributeField::Ignored | AttributeField::Pointer
                ) {
                    return false;
                }
                let Some(&referenced_xmt) = entity.trailing_references.values().get(field_ordinal)
                else {
                    return true;
                };
                if referenced_xmt == 1 {
                    return false;
                }
                let Ok(field_ordinal) = u32::try_from(field_ordinal) else {
                    return true;
                };
                !matches!(
                    fields_by_identity
                        .get(&(entity.id.as_str(), field_ordinal))
                        .map(Vec::as_slice),
                    Some([_])
                )
            })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn unicode_record_wire_derives_exact_utf16_and_rejects_disagreement() {
        let wire = r#"{"id":"unicode","stream_ordinal":0,"xmt":2,"code_units":[78,88,55357,56960],"value":"NX🚀","byte_len":8,"inflated_offset":0}"#;
        let record: super::ParasolidEntity62UnicodeRecord = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), wire);
        let inconsistent = wire.replace("[78,88,55357,56960]", "[78,88,55357]");
        assert!(
            serde_json::from_str::<super::ParasolidEntity62UnicodeRecord>(&inconsistent).is_err()
        );
    }

    use std::io::Cursor;

    use crate::parasolid::Stream;
    use crate::test_support::many_face_partition_stream;
    use crate::topology::Graph;

    #[test]
    fn type38_leading_statuses_preserve_default_omission_and_nondefault_values() {
        use crate::deltas::inline_schema_fields::InlineSchemaFields;
        let base = serde_json::json!({
            "schema": "type38", "xmt": 3, "node_id": 7,
            "leading_references": [1, 2, 3, 4, 5], "marker": 45,
            "linked_references": [2, 3], "state_references": [6, 7, 8], "numeric_values": null
        });
        for statuses in [None, Some([1; 5]), Some([1, 1, 1, 1, 0])] {
            let mut wire = base.clone();
            if let Some(statuses) = statuses {
                wire["leading_statuses"] = serde_json::json!(statuses);
            }
            let fields: InlineSchemaFields = serde_json::from_value(wire.clone()).unwrap();
            let InlineSchemaFields::Type38 { state } = &fields else {
                panic!("type38 wire must decode as Type38");
            };
            assert_eq!(state.leading_statuses(), statuses.unwrap_or([1; 5]));
            if state.leading_statuses() == [1; 5] {
                wire.as_object_mut().unwrap().remove("leading_statuses");
            }
            assert_eq!(serde_json::to_value(fields).unwrap(), wire);
        }
        let mut null = base.clone();
        null["leading_statuses"] = serde_json::Value::Null;
        let fields: InlineSchemaFields = serde_json::from_value(null).unwrap();
        assert_eq!(serde_json::to_value(fields).unwrap(), base);
    }

    fn group_record(xmt: u16, node_id: u32, linked_reference: u16) -> Vec<u8> {
        let mut bytes = vec![0, 90];
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&node_id.to_be_bytes());
        for reference in [3u16, 4, 5, 6] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(4);
        bytes.extend_from_slice(&linked_reference.to_be_bytes());
        bytes.push(0);
        bytes
    }

    fn stream(
        subtype: crate::parasolid::ParasolidSubtype,
        schema: &str,
        inflated: Vec<u8>,
    ) -> Stream {
        Stream {
            file_offset: 0,
            consumed: 0,
            inflated,
            body: crate::parasolid::StreamBody::Parasolid {
                subtype,
                schema: Some(schema.to_string()),
            },
        }
    }

    #[test]
    fn native_value_records_use_only_ledger_owned_offsets() {
        let mut outer = vec![0x00, 0x52];
        outer.extend_from_slice(&4u32.to_be_bytes());
        outer.extend_from_slice(&10u16.to_be_bytes());
        outer.extend_from_slice(&[0x00, 0x53]);
        outer.extend_from_slice(&1u32.to_be_bytes());
        outer.extend_from_slice(&20u16.to_be_bytes());
        outer.extend_from_slice(&0.25f64.to_be_bytes());

        let streams = [stream(
            crate::parasolid::ParasolidSubtype::Deltas,
            "SCH_TEST",
            outer,
        )];
        let events = super::parasolid_deltas_events(&streams);
        let records = super::parasolid_entity_value_records(&streams, &events.records);

        assert_eq!(records.integers.len(), 1);
        assert_eq!(records.integers[0].values.as_slice().len(), 4);
        assert!(records.doubles.is_empty());
    }

    fn record(
        kind: u16,
        xmt: u32,
        node_id: Option<u32>,
        references: Vec<u32>,
    ) -> crate::deltas::Record {
        use crate::deltas::record_family::RecordFamily;
        let family = match kind {
            14 => RecordFamily::Face {
                references: [1; 11],
                node_id: node_id.expect("face node"),
            },
            16 => RecordFamily::Edge {
                references: [1; 8],
                node_id: node_id.expect("edge node"),
            },
            90 => RecordFamily::Group {
                references: references.try_into().unwrap(),
                node_id: node_id.expect("group node"),
                selector: GroupSelector::Form4,
                linked_reference_status: GroupReferenceStatus::Form0,
            },
            91 => RecordFamily::Type91 {
                references: references.try_into().unwrap(),
            },
            other => panic!("unexpected test kind {other}"),
        };
        crate::deltas::Record {
            family,
            xmt,
            canonical_bytes: if kind == 90 { vec![0, 90] } else { Vec::new() },
            offset: 0,
            end: 1,
        }
    }

    #[test]
    fn group_members_follow_complete_bidirectional_type_91_chain() {
        let group = record(90, 10, Some(7), vec![3, 4, 5, 6, 30]);
        let tail = record(91, 30, None, vec![10, 100, 3, 4, 20, 1]);
        let head = record(91, 20, None, vec![10, 101, 3, 4, 1, 30]);
        let tail_member = record(14, 100, Some(50), Vec::new());
        let head_member = record(16, 101, Some(51), Vec::new());
        let records = [group, tail, head, tail_member, head_member];

        let members = super::group_members_from_records(4, &records);

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
        assert!(super::group_members_from_records(4, &broken).is_empty());
    }

    #[test]
    fn group_member_xmt_is_checked_before_node_identity_fallback() {
        let graph = Graph::parse(&many_face_partition_stream(1_000));
        let resolve = |member: &ParasolidGroupMember| match member
            .target
            .resolve(&graph, member.member_xmt)
        {
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

        let groups = super::parasolid_group_records(&streams, &BTreeMap::new(), &[]);

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
        let events = super::parasolid_deltas_events(&streams);
        let pairs = BTreeMap::from([(0, vec![1])]);

        let groups = super::parasolid_group_records(&streams, &pairs, &events.records);

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].origin.partition_stream_ordinal(), Some(0));
        assert_eq!(groups[1].origin.partition_stream_ordinal(), Some(0));
        assert_eq!(groups[2].origin.partition_stream_ordinal(), None);
        assert_eq!(groups[1].origin.stream_kind().label(), "deltas");
    }

    fn deltas_type_45(xmt: u16) -> Vec<u8> {
        let mut bytes = 45u16.to_be_bytes().to_vec();
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&1.25f64.to_be_bytes());
        bytes.extend_from_slice(&2.5f64.to_be_bytes());
        bytes
    }

    #[test]
    fn deltas_events_retain_bounded_records_tombstones_and_revisions() {
        let mut bytes = vec![0xaa, 0xbb, 0xcc, 0, 12, 0, 3];
        bytes.extend_from_slice(&9u32.to_be_bytes());
        for reference in [2u16, 3, 4, 5, 6, 7, 8, 9] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        let revision_prefix_end = bytes.len();
        let revision_state_tail = [0xde, 0xad, 0xbe, 0xef];
        bytes.extend_from_slice(&revision_state_tail);
        let type_45_offset = bytes.len();
        bytes.extend_from_slice(&45u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&1.25f64.to_be_bytes());
        bytes.extend_from_slice(&2.5f64.to_be_bytes());
        bytes.extend_from_slice(&[0xdd, 0xee]);
        let tombstone_offset = bytes.len();
        bytes.extend_from_slice(&[0, 29, 0, 11, 0, 1]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes,
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let census = crate::deltas::census::walk(&streams[0].inflated);
        let events = super::parasolid_deltas_events_with_censuses(&streams, vec![Some(census)]);

        assert_eq!(events.body_revisions.len(), 1);
        assert_eq!(u32::from(events.body_revisions[0].xmt), 3);
        assert_eq!(events.body_revisions[0].node_id, 9);
        assert_eq!(
            events.body_revisions[0].references,
            [2, 3, 4, 5, 6, 7, 8, 9]
        );
        assert_eq!(events.body_revisions[0].lengths.prefix(), 32);
        assert_eq!(events.body_revisions[0].lengths.tail(), 4);
        assert_eq!(events.body_revisions[0].lengths.total(), 36);
        assert_eq!(
            events.body_revisions[0].state_tail_sha256,
            crate::native::hex::Sha256Hex::digest(&revision_state_tail)
        );
        assert_eq!(
            events.body_revisions[0].inflated_offset + events.body_revisions[0].lengths.prefix(),
            revision_prefix_end as u64
        );
        assert_eq!(events.records.len(), 1);
        assert_eq!(events.records[0].family.family_name(), "TYPE_45");
        assert_eq!(events.records[0].xmt, 10);
        assert_eq!(events.records[0].inflated_offset, type_45_offset as u64);
        assert_eq!(events.records[0].byte_len, 24);
        assert_eq!(events.tombstones.len(), 1);
        assert_eq!(events.tombstones[0].kind.name(), "POINT");
        assert_eq!(events.tombstones[0].xmt, 11);
        assert_eq!(
            serde_json::to_value(&events.tombstones[0]).unwrap()["byte_len"],
            6
        );
        assert_eq!(
            events.tombstones[0].inflated_offset,
            tombstone_offset as u64
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].inflated_offset, 0);
        assert_eq!(events.residual_spans[0].byte_len, 3);
        assert_eq!(
            events.residual_spans[0].sha256,
            crate::native::hex::Sha256Hex::digest(&[0xaa, 0xbb, 0xcc])
        );
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            (type_45_offset + 24) as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_typed_term_use_numeric_tails_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend_from_slice(&41u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&20u16.to_be_bytes());
        bytes.extend_from_slice(b"L?");
        for coordinate in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&coordinate.to_be_bytes());
        }
        let tail_offset = bytes.len();
        for ordinal in 0..8 {
            bytes.extend_from_slice(&(ordinal as f64 + 0.5).to_be_bytes());
        }
        bytes.extend_from_slice(&[0xcc, 0xdd, 0xee]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes,
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.term_use_numeric_tails.len(), 1);
        let tail = &events.term_use_numeric_tails[0];
        assert_eq!(tail.term_use_xmt, 20);
        assert_eq!(tail.values.term_use_count(), 1);
        assert_eq!(tail.values.values().len(), 8);
        assert_eq!(tail.values.byte_len(), 64);
        assert_eq!(tail.inflated_offset, tail_offset as u64);
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            (tail_offset + 64) as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 3);
    }

    #[test]
    fn deltas_events_subtract_tagged_reference_lanes_from_residuals() {
        let mut bytes = [0xaa, 0xbb, 0xcc].to_vec();
        bytes.extend(deltas_type_45(10));
        let lane_offset = bytes.len();
        bytes.extend([
            0x00, 0x4f, 0x00, 0x0a, // direct type-79 reference
            0x00, 0x50, 0xff, 0xff, 0x00, 0x01, // extended type-80 reference
        ]);
        let lane_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xdd, 0xee]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.tagged_reference_lanes.len(), 1);
        let lane = &events.tagged_reference_lanes[0];
        assert_eq!(
            Vec::<(u16, u32)>::from(lane.references.clone()),
            [(79, 10), (80, 32_768)]
        );
        assert_eq!(lane.byte_len, 10);
        assert_eq!(lane.inflated_offset, lane_offset as u64);
        assert_eq!(
            lane.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[lane_offset..lane_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].inflated_offset, 0);
        assert_eq!(events.residual_spans[0].byte_len, 3);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_transmit_headers_from_residuals() {
        let description = b": TRANSMIT FILE (deltas) created by modeller version 3501171";
        let schema = b"SCH_3501171_35102_13006";
        let mut bytes = b"PS".to_vec();
        bytes.extend_from_slice(&(description.len() as u32).to_be_bytes());
        bytes.extend_from_slice(description);
        bytes.extend_from_slice(&(schema.len() as u32).to_be_bytes());
        bytes.extend_from_slice(schema);
        bytes.extend_from_slice(&[
            0, 0xe7, 0, 0, 0, 0, 0, 3, 0xff, 0x04, 0x27, 0x04, 0x28, 0, 0,
        ]);
        let header_end = bytes.len();
        bytes.extend([0xaa, 0xbb]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: Some("SCH_3501171_35102_13006".to_string()),
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.transmit_headers.len(), 1);
        let header = &events.transmit_headers[0];
        assert_eq!(header.id, "nx:s0:deltas-transmit-header#0");
        assert_eq!(header.state.description().as_bytes(), description);
        assert_eq!(header.state.schema().as_bytes(), schema);
        assert_eq!(header.state.references(), [1063, 1064]);
        assert_eq!(header.byte_len, header_end as u64);
        assert_eq!(
            header.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[..header_end])
        );
        assert_eq!(events.residual_spans.len(), 1);
        assert_eq!(events.residual_spans[0].inflated_offset, header_end as u64);
        assert_eq!(events.residual_spans[0].byte_len, 2);
    }

    #[test]
    fn deltas_events_retain_terminal_null_references() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        let trailer_offset = bytes.len();
        bytes.extend_from_slice(&[0, 1, 0, 1, 0, 1, 0, 1]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.terminal_null_references.len(), 1);
        let trailer = &events.terminal_null_references[0];
        assert_eq!(trailer.form.references(), [1; 4]);
        assert_eq!(trailer.form.raw().len(), 8);
        assert_eq!(trailer.inflated_offset, trailer_offset as u64);
        assert_eq!(
            serde_json::to_value(trailer).unwrap()["sha256"],
            serde_json::json!(
                crate::native::hex::Sha256Hex::digest(&bytes[trailer_offset..]).as_str()
            )
        );
        assert_eq!(events.residual_spans.len(), 1);
        assert_eq!(events.residual_spans[0].inflated_offset, 0);
        assert_eq!(events.residual_spans[0].byte_len, trailer_offset as u64);
    }

    #[test]
    fn deltas_events_subtract_reference_type_maps_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let map_offset = bytes.len();
        bytes.extend([
            0, 1, 0, 1, 0xe3, 0xbf, 0, 1, 0, 81, 0, 3, 0, 100, 0, 1, 0, 0, 0, 55,
        ]);
        let map_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.reference_type_maps.len(), 1);
        let map = &events.reference_type_maps[0];
        assert_eq!(
            Vec::<(u32, u16)>::from(map.entries.clone()),
            [(40_000, 81), (3, 100)]
        );
        assert_eq!(map.target_kind.map(std::num::NonZeroU16::get), Some(55));
        assert_eq!(map.byte_len, 20);
        assert_eq!(map.inflated_offset, map_offset as u64);
        assert_eq!(
            map.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[map_offset..map_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_reference_state_packets_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let packet_offset = bytes.len();
        bytes.extend([0, 1, 0, 1, 0, 4]);
        for reference in [2u16, 3, 4, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&1u16.to_be_bytes());
        for word in [34u32, 6, 11, 22_362, 1] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes.push(65);
        let packet_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd, 0xee]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.reference_state_packets.len(), 1);
        let packet = &events.reference_state_packets[0];
        assert_eq!(
            packet.frames.as_slice(),
            [crate::deltas::state_frame::ReferenceStateFrame {
                references: [2, 3, 4, 1].try_into().unwrap(),
                state_words: [34, 6, 11, 22_362, 1],
                state_byte: 65,
            }]
        );
        assert!(!packet.terminal);
        assert_eq!(packet.byte_len, 37);
        assert_eq!(packet.inflated_offset, packet_offset as u64);
        assert_eq!(
            packet.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[packet_offset..packet_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 3);
    }

    #[test]
    fn deltas_events_retain_schema_reference_preambles() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let preamble_offset = bytes.len();
        bytes.extend_from_slice(&300u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.push(0xff);
        for reference in [2u16, 3, 1, 1, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for state_word in [2u32, 0, 1, 55] {
            bytes.extend_from_slice(&state_word.to_be_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&300u16.to_be_bytes());
        for reference in [1u16, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&5u16.to_be_bytes());
        for (kind, reference) in [(81u16, 4u16), (82, 5), (81, 6)] {
            bytes.extend_from_slice(&kind.to_be_bytes());
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&82u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&9u16.to_be_bytes());
        let preamble_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.schema_reference_preambles.len(), 1);
        let preamble = &events.schema_reference_preambles[0];
        assert_eq!(preamble.state.identity(), 300);
        assert_eq!(preamble.state.references(), [2, 3]);
        assert_eq!(preamble.state.state_reference(), None);
        assert_eq!(preamble.state.state_words(), [2, 0, 1, 55]);
        assert_eq!(preamble.state.count(), 5);
        assert_eq!(preamble.state.entries(), [(81, 4), (82, 5), (81, 6)]);
        assert_eq!(preamble.state.terminal_value(), 9);
        assert_eq!(preamble.inflated_offset, preamble_offset as u64);
        assert_eq!(preamble.byte_len, (preamble_end - preamble_offset) as u64);
        assert_eq!(
            preamble.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[preamble_offset..preamble_end])
        );
    }

    #[test]
    fn deltas_events_subtract_reference_marker_packets_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let packet_offset = bytes.len();
        bytes.extend([0, 9, 1, 0, 1, 1, 0x53, 0, 1, 1]);
        let packet_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.reference_marker_packets.len(), 1);
        let packet = &events.reference_marker_packets[0];
        assert_eq!(u32::from(packet.reference), 9);
        assert_eq!(u8::from(packet.marker), 0x53);
        assert_eq!(packet.byte_len, 10);
        assert_eq!(packet.inflated_offset, packet_offset as u64);
        assert_eq!(
            packet.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[packet_offset..packet_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn deltas_events_subtract_inline_schema_declarations_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let declaration_offset = bytes.len();
        bytes.extend([
            0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61,
            0x6d, 0x65, 0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72,
            0x00, 0x0c, 0x00, 0x01, 0x5a,
        ]);
        bytes.extend_from_slice(&11u16.to_be_bytes());
        bytes.extend_from_slice(&5u32.to_be_bytes());
        for reference in [1u16, 3, 1, 9] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        let declaration_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.inline_schema_declarations.len(), 1);
        let declaration = &events.inline_schema_declarations[0];
        assert_eq!(
            declaration.fields,
            InlineSchemaFields::Region {
                xmt: 11u32.try_into().unwrap(),
                state_word: 5,
                references: [1, 3, 1, 9],
            }
        );
        assert_eq!(declaration.byte_len, 51);
        assert_eq!(declaration.inflated_offset, declaration_offset as u64);
        assert_eq!(
            declaration.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[declaration_offset..declaration_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition,
        SurfaceGeometry,
    };

    use cadmpeg_ir::report::LossCategory;

    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn attribute_value_uses_are_assigned_to_compatible_declared_fields() {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 2,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(9).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(10).unwrap(),
            identifier_inflated_offset: 32,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: None,
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![
                AttributeField::Integer,
                AttributeField::Real,
                AttributeField::Character,
            ],
            inflated_offset: 40,
        };
        let class_use = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "nx:s2:attribute-class-use#class-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: NonNullXmt::try_from(9).unwrap(),
            attribute_definition: "definition".into(),
        };
        let numeric_use = ParasolidEntity51NumericUse {
            id: "numeric-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            referenced_xmt: NonNullXmt::try_from(12).unwrap(),
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: "integers".into(),
            inflated_offset: 48,
        };
        let double_use = ParasolidEntity51NumericUse {
            id: "double-use".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(6).unwrap(),
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: "doubles".into(),
            ..numeric_use.clone()
        };
        let string_use = ParasolidEntity51StringUse {
            id: "string-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(7).unwrap(),
            referenced_xmt: NonNullXmt::try_from(14).unwrap(),
            string_record: "string".into(),
            inflated_offset: 48,
        };

        let uses = parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[numeric_use.clone(), double_use],
            std::slice::from_ref(&string_use),
            &[],
        );

        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].id, "nx:s2:attribute-field-use#class-use-0");
        assert_eq!(
            uses[0].attribute_class_use,
            "nx:s2:attribute-class-use#class-use"
        );
        assert_eq!(uses[0].attribute_definition, "definition");
        assert_eq!(uses[0].position.field_ordinal(), 0);
        assert_eq!(uses[0].value_kind.field_code().code(), 1);
        assert_eq!(uses[0].position.reference_ordinal(), 5);
        assert_eq!(
            uses[0].value_kind,
            ParasolidAttributeFieldValueKind::UnsignedIntegers
        );
        assert_eq!(uses[0].value_use, "numeric-use");
        assert_eq!(uses[0].value_record, "integers");
        assert_eq!(uses[1].position.field_ordinal(), 1);
        assert_eq!(uses[1].value_kind.field_code().code(), 2);
        assert_eq!(
            uses[1].value_kind,
            ParasolidAttributeFieldValueKind::Doubles
        );
        assert_eq!(uses[1].value_record, "doubles");
        assert_eq!(uses[2].position.field_ordinal(), 2);
        assert_eq!(uses[2].value_kind.field_code().code(), 3);
        assert_eq!(uses[2].value_kind, ParasolidAttributeFieldValueKind::String);
        assert_eq!(uses[2].value_record, "string");

        let duplicate = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "duplicate".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: NonNullXmt::try_from(11).unwrap(),
            attribute_definition: "other-definition".into(),
        };
        assert!(parasolid_attribute_field_uses(
            &[class_use.clone(), duplicate.clone()],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[],
            &[],
        )
        .is_empty());

        let wrong_stream = ParasolidAttributeClassUse {
            stream_ordinal: 3,
            ..duplicate
        };
        assert!(parasolid_attribute_field_uses(
            &[wrong_stream],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[],
            &[],
        )
        .is_empty());

        let mismatched = ParasolidEntity51NumericUse {
            kind: ParasolidEntity51NumericKind::Doubles,
            ..numeric_use.clone()
        };
        assert!(parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[mismatched],
            &[],
            &[],
        )
        .is_empty());

        let ambiguous_string = ParasolidEntity51StringUse {
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            ..string_use
        };
        assert!(parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&numeric_use),
            &[ambiguous_string],
            &[],
        )
        .is_empty());
    }

    #[test]
    fn structured_value_uses_require_one_same_stream_family() {
        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 2,
            xmt: NonNullXmt::try_from(10).unwrap(),
            sequence: NonZeroU32::new(1).unwrap(),
            definition_xmt: 9,
            leading_references: [1; 5],
            trailing_references: EntityReferences::new(vec![12]).unwrap(),
            byte_len: 32,
            inflated_offset: 40,
        };
        let point = ParasolidEntityVectorRecord {
            id: "point".into(),
            stream_ordinal: 2,
            kind: ParasolidVectorValueKind::Points,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(12).unwrap(),
            values: crate::parasolid::counted_values::CountedValues::new(vec![[1.0, 2.0, 3.0]])
                .unwrap(),
            byte_len: 36,
            inflated_offset: 80,
        };
        let uses = parasolid_entity_51_structured_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&point),
            &[],
            &[],
            &[],
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].position.reference_ordinal(), 5);
        assert_eq!(uses[0].kind, StructuredValueKind::Points);
        assert_eq!(uses[0].value_record, "point");

        let colliding_tag = ParasolidEntity58TagRecord {
            id: "tag".into(),
            stream_ordinal: 2,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(12).unwrap(),
            values: crate::parasolid::counted_values::CountedValues::new(vec![7]).unwrap(),
            byte_len: 16,
            inflated_offset: 90,
        };
        assert!(parasolid_entity_51_structured_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&point),
            &[],
            std::slice::from_ref(&colliding_tag),
            &[],
        )
        .is_empty());

        let other_stream = ParasolidEntityVectorRecord {
            stream_ordinal: 3,
            ..point
        };
        assert!(
            parasolid_entity_51_structured_uses(&[entity], &[other_stream], &[], &[], &[],)
                .is_empty()
        );
    }

    #[test]
    fn structured_value_families_match_only_their_declared_field_codes() {
        let kinds = [
            StructuredValueKind::Points,
            StructuredValueKind::Vectors,
            StructuredValueKind::Directions,
            StructuredValueKind::Axes,
            StructuredValueKind::Tags,
            StructuredValueKind::Unicode,
        ];
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 2,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(9).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(10).unwrap(),
            identifier_inflated_offset: 32,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: None,
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![
                AttributeField::Point,
                AttributeField::Vector,
                AttributeField::Direction,
                AttributeField::Axis,
                AttributeField::Tag,
                AttributeField::Unicode,
            ],
            inflated_offset: 40,
        };
        let class_use = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "nx:s2:attribute-class-use#class-use".into(),
            stream_ordinal: 2,
            entity_51_record: "entity".into(),
            definition_xmt: NonNullXmt::try_from(9).unwrap(),
            attribute_definition: definition.id.clone(),
        };
        let structured = kinds
            .iter()
            .enumerate()
            .map(|(ordinal, kind)| ParasolidEntity51StructuredUse {
                id: format!("use-{ordinal}"),
                stream_ordinal: 2,
                entity_51_record: "entity".into(),
                position: crate::parasolid::entity_references::FieldPosition::try_from(
                    u32::try_from(ordinal).expect("test ordinal fits u32") + 5,
                )
                .unwrap(),
                referenced_xmt: NonNullXmt::try_from(
                    u32::try_from(ordinal).expect("test ordinal fits u32") + 20,
                )
                .unwrap(),
                kind: *kind,
                value_record: format!("value-{ordinal}"),
                inflated_offset: 48,
            })
            .collect::<Vec<_>>();
        let uses = parasolid_attribute_field_uses(
            std::slice::from_ref(&class_use),
            std::slice::from_ref(&definition),
            &[],
            &[],
            &structured,
        );
        assert_eq!(
            uses.iter().map(|use_| use_.value_kind).collect::<Vec<_>>(),
            kinds.map(ParasolidAttributeFieldValueKind::from)
        );

        let mut mismatched = structured;
        mismatched[0].kind = StructuredValueKind::Vectors;
        let uses =
            parasolid_attribute_field_uses(&[class_use], &[definition], &[], &[], &mismatched);
        assert_eq!(uses.len(), 5);
        assert!(uses.iter().all(|use_| use_.position.field_ordinal() != 0));
    }

    #[test]
    fn attribute_loss_requires_concrete_unresolved_references() {
        let definition = |field_names_xmt, field_codes: Vec<u8>| ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 0,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(20).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(21).unwrap(),
            identifier_inflated_offset: 10,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: XmtTarget::from_wire(field_names_xmt),
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: field_codes
                .into_iter()
                .map(|code| AttributeField::try_from(code).unwrap())
                .collect(),
            inflated_offset: 20,
        };

        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 0,
            xmt: NonNullXmt::try_from(30).unwrap(),
            sequence: NonZeroU32::new(1).unwrap(),
            definition_xmt: 20,
            leading_references: [1; 5],
            trailing_references: EntityReferences::new(vec![40]).unwrap(),
            byte_len: 32,
            inflated_offset: 30,
        };
        let class_use = ParasolidAttributeClassUse {
            inflated_offset: 30,
            id: "class-use".into(),
            stream_ordinal: 0,
            entity_51_record: entity.id.clone(),
            definition_xmt: NonNullXmt::try_from(20).unwrap(),
            attribute_definition: "definition".into(),
        };
        let field_use = ParasolidAttributeFieldUse {
            id: "field-use".into(),
            stream_ordinal: 0,
            attribute_class_use: class_use.id.clone(),
            entity_51_record: entity.id.clone(),
            attribute_definition: "definition".into(),
            position: crate::parasolid::entity_references::FieldPosition::try_from(5).unwrap(),
            value_kind: ParasolidAttributeFieldValueKind::Points,
            value_use: "point-use".into(),
            value_record: "points".into(),
            inflated_offset: 30,
        };
        let topology_reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 0,
            topology_type: TopologyAttributeKind::Face,
            topology_xmt: 50,
            attribute_list_xmt: entity.xmt.into(),
            attribute_list_record: Some(entity.id.clone()),
            inflated_offset: 28,
        };
        let topology_class_use = ParasolidTopologyAttributeClassUse {
            stream_ordinal: topology_reference.stream_ordinal,
            inflated_offset: entity.inflated_offset,
            id: "topology-class-use".into(),
            topology_attribute_reference: topology_reference.id.clone(),
            entity_51_record: entity.id.clone(),
            attribute_class_use: class_use.id.clone(),
            definition_xmt: NonNullXmt::try_from(20).unwrap(),
            attribute_definition: "definition".into(),
        };

        // An unused declaration carries no value-loss evidence.
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                &[],
                &[],
                &[],
            )
        );
        // A non-null instance reference must have exactly one resolved field use.
        assert!(
            parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                std::slice::from_ref(&entity),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&topology_class_use),
            )
        );
        // Null values and always-empty pointer fields require no value relation.
        let mut null_entity = entity.clone();
        null_entity.trailing_references.values_mut()[0] = 1;
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![4])],
                &[null_entity],
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                &[definition(1, vec![9])],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );

        let named_definition = definition(22, vec![4]);
        // An unresolved optional field-name list uses the specification's
        // deterministic ordinal/code fallback and does not lose the value.
        assert!(
            !parasolid_topology_attribute_fields_have_untransferred_values(
                std::slice::from_ref(&named_definition),
                std::slice::from_ref(&entity),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&topology_class_use),
            )
        );
        assert!(
            parasolid_topology_attribute_fields_have_untransferred_values(
                &[named_definition],
                std::slice::from_ref(&entity),
                &[],
                std::slice::from_ref(&topology_class_use),
            )
        );
    }

    #[test]
    fn attribute_field_names_require_complete_unambiguous_same_stream_relations() {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(20).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(21).unwrap(),
            identifier_inflated_offset: 10,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: XmtTarget::from_wire(25),
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![
                AttributeField::Real,
                AttributeField::Integer,
                AttributeField::Integer,
            ],
            inflated_offset: 20,
        };
        let list = ParasolidFieldNamesRecord {
            id: "field-names".into(),
            stream_ordinal: 3,
            xmt: NonNullXmt::try_from(25).unwrap(),
            name_xmts: NameReferences::try_from(
                [28, 29, 30]
                    .map(|xmt| NonNullXmt::try_from(xmt).unwrap())
                    .to_vec(),
            )
            .unwrap(),
            byte_len: 15,
            inflated_offset: 30,
        };
        let strings = [28, 30].map(|xmt| ParasolidEntity54StringRecord {
            id: format!("string-{xmt}"),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(xmt).unwrap(),
            value: PrintableString::new((xmt - 27).to_string()).unwrap(),
            byte_len: 10,
            inflated_offset: u64::from(xmt),
        });
        let unicode = ParasolidEntity62UnicodeRecord {
            id: "unicode-29".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(29).unwrap(),
            value: crate::parasolid::unicode_value::UnicodeValue::new("μ".into()).unwrap(),
            byte_len: 12,
            inflated_offset: 29,
        };

        let relations = parasolid_attribute_field_names(
            std::slice::from_ref(&definition),
            std::slice::from_ref(&list),
            &strings,
            std::slice::from_ref(&unicode),
        );
        assert_eq!(relations.len(), 1);
        assert_eq!(
            relations[0]
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["1", "μ", "3"]
        );

        let mut incomplete = list.clone();
        incomplete.name_xmts =
            NameReferences::try_from(incomplete.name_xmts.as_slice()[..2].to_vec()).unwrap();
        assert!(parasolid_attribute_field_names(
            std::slice::from_ref(&definition),
            &[incomplete],
            &strings,
            std::slice::from_ref(&unicode),
        )
        .is_empty());
        assert!(parasolid_attribute_field_names(
            &[definition.clone(), definition.clone()],
            std::slice::from_ref(&list),
            &strings,
            std::slice::from_ref(&unicode),
        )
        .is_empty());

        let ambiguous = ParasolidEntity62UnicodeRecord {
            xmt: NonNullXmt::try_from(28).unwrap(),
            ..unicode.clone()
        };
        assert!(parasolid_attribute_field_names(
            std::slice::from_ref(&definition),
            &[ParasolidFieldNamesRecord {
                id: "field-names".into(),
                stream_ordinal: 3,
                xmt: NonNullXmt::try_from(25).unwrap(),
                name_xmts: NameReferences::try_from(
                    [28, 29, 30]
                        .map(|xmt| NonNullXmt::try_from(xmt).unwrap())
                        .to_vec()
                )
                .unwrap(),
                byte_len: 15,
                inflated_offset: 30,
            }],
            &strings,
            &[unicode, ambiguous],
        )
        .is_empty());
    }

    #[test]
    fn deltas_events_subtract_type_150_state_packets_from_residuals() {
        let mut bytes = [0xaa, 0xbb].to_vec();
        bytes.extend(deltas_type_45(10));
        let packet_offset = bytes.len();
        bytes.push(150);
        for (reference, status) in [(1u16, 1), (3, 1), (6_192, 0), (6_193, 1), (6_194, 0)] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(status);
        }
        bytes.push(0x2b);
        let values: [f64; 9] = [-0.025, -0.05, 0.25, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0];
        for value in values {
            bytes.extend(value.to_be_bytes());
        }
        let packet_end = bytes.len();
        bytes.extend(deltas_type_45(11));
        let suffix_offset = bytes.len();
        bytes.extend([0xcc, 0xdd]);
        let streams = [Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes.clone(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        }];

        let events = super::parasolid_deltas_events(&streams);

        assert_eq!(events.type_150_state_packets.len(), 1);
        let packet = &events.type_150_state_packets[0];
        assert_eq!(packet.state.references(), [1, 3, 6_192, 6_193, 6_194]);
        assert_eq!(u8::from(packet.state.marker), 0x2b);
        assert_eq!(*packet.state.values(), values);
        assert_eq!(packet.inflated_offset, packet_offset as u64);
        assert_eq!(packet.byte_len, (packet_end - packet_offset) as u64);
        assert_eq!(
            packet.sha256,
            crate::native::hex::Sha256Hex::digest(&bytes[packet_offset..packet_end])
        );
        assert_eq!(events.residual_spans.len(), 2);
        assert_eq!(events.residual_spans[0].byte_len, 2);
        assert_eq!(
            events.residual_spans[1].inflated_offset,
            suffix_offset as u64
        );
        assert_eq!(events.residual_spans[1].byte_len, 2);
    }

    #[test]
    fn topology_retains_entity_attribute_list_references() {
        let mut stream = topology_partition_stream();
        for (kind, attribute) in [(14, 41), (15, 42), (17, 43), (16, 44), (18, 45)] {
            let at = stream
                .windows(2)
                .position(|window| window == [0, kind])
                .expect("topology record");
            put_ref(&mut stream, at + if kind == 17 { 4 } else { 8 }, attribute);
        }
        stream.extend_from_slice(&[0, 0x51]);
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&41u16.to_be_bytes());
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in [4u16, 1, 1, 1, 1, 42] {
            stream.extend_from_slice(&reference.to_be_bytes());
        }
        stream.extend_from_slice(&[0, 0x54]);
        stream.extend_from_slice(&8u32.to_be_bytes());
        stream.extend_from_slice(&42u16.to_be_bytes());
        stream.extend_from_slice(b"deadbeef\0");

        let graph = crate::topology::Graph::parse(&stream);
        assert_eq!(
            graph
                .get(crate::framing::node_kind::NodeKind::Face, 4)
                .expect("required invariant")
                .face_fields()
                .expect("required invariant")
                .attributes
                .map(u32::from)
                .expect("non-null attribute target"),
            41
        );
        assert_eq!(
            graph
                .get(crate::framing::node_kind::NodeKind::Loop, 5)
                .expect("required invariant")
                .loop_fields()
                .expect("required invariant")
                .attributes
                .map(u32::from)
                .expect("non-null attribute target"),
            42
        );
        assert_eq!(
            graph
                .get(crate::framing::node_kind::NodeKind::Fin, 7)
                .expect("required invariant")
                .fin_fields()
                .expect("required invariant")
                .attributes
                .map(u32::from)
                .expect("non-null attribute target"),
            43
        );
        assert_eq!(
            graph
                .get(crate::framing::node_kind::NodeKind::Edge, 8)
                .expect("required invariant")
                .edge_fields()
                .expect("required invariant")
                .attributes
                .map(u32::from)
                .expect("non-null attribute target"),
            44
        );
        assert_eq!(
            graph
                .get(crate::framing::node_kind::NodeKind::Vertex, 10)
                .expect("required invariant")
                .vertex_fields()
                .expect("required invariant")
                .attributes
                .map(u32::from)
                .expect("non-null attribute target"),
            45
        );

        let result = NxCodec
            .decode(
                &mut Cursor::new(prt_with_partition(&stream)),
                &DecodeOptions::default(),
            )
            .expect("required invariant");
        let references = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::ParasolidTopologyAttributeListReference>(
                "parasolid_topology_attribute_list_references",
            )
            .expect("required invariant");
        assert_eq!(references.len(), 5);
        assert_eq!(references[0].topology_type.code(), 14);
        assert_eq!(references[0].topology_xmt, 4);
        assert_eq!(references[0].attribute_list_xmt, 41);
        assert!(references[0].attribute_list_record.is_some());
        assert_eq!(result.ir().model.attributes.len(), 1);
        assert_eq!(
            result.ir().model.attributes[0].target,
            cadmpeg_ir::attributes::AttributeTarget::Face(
                cadmpeg_ir::ids::FaceId::mint("nx:s0:face#4").expect("identity grammar")
            )
        );
        assert_eq!(
            result.ir().model.attributes[0].name,
            "parasolid_type_84_reference_5"
        );
        assert_eq!(
            result.ir().model.attributes[0].values,
            [cadmpeg_ir::attributes::AttributeValue::String(
                "deadbeef".into()
            )]
        );
    }

    #[test]
    fn topology_attribute_class_uses_resolve_type_80_definitions_by_xmt() {
        use super::{
            ParasolidAttributeDefinition, ParasolidEntity51Record,
            ParasolidTopologyAttributeListReference,
        };

        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(34).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(35).unwrap(),
            identifier_inflated_offset: 80,
            name: crate::printable_string::PrintableString::new("UG2/PMARK_ATTRIBUTE".to_string())
                .unwrap(),
            type_id: std::num::NonZeroU32::new(9000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: None,
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![AttributeField::Integer],
            inflated_offset: 100,
        };
        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 3,
            xmt: NonNullXmt::try_from(50).unwrap(),
            sequence: NonZeroU32::new(7).unwrap(),
            definition_xmt: 34,
            leading_references: [60, 61, 1, 62, 63],
            trailing_references: EntityReferences::new(vec![64]).unwrap(),
            byte_len: 26,
            inflated_offset: 200,
        };
        let reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 3,
            topology_type: TopologyAttributeKind::Face,
            topology_xmt: 60,
            attribute_list_xmt: 50,
            attribute_list_record: Some(entity.id.clone()),
            inflated_offset: 300,
        };

        let instance_uses = super::parasolid_attribute_class_uses(
            std::slice::from_ref(&entity),
            std::slice::from_ref(&definition),
        );
        assert_eq!(instance_uses.len(), 1);
        assert_eq!(instance_uses[0].entity_51_record, entity.id);
        assert_eq!(u32::from(instance_uses[0].definition_xmt), 34);
        assert_eq!(instance_uses[0].attribute_definition, definition.id);

        let uses = super::parasolid_topology_attribute_class_uses(
            std::slice::from_ref(&reference),
            std::slice::from_ref(&entity),
            &instance_uses,
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].attribute_class_use, instance_uses[0].id);
        assert_eq!(u32::from(uses[0].definition_xmt), 34);
        assert_eq!(uses[0].attribute_definition, definition.id);
        assert!(super::parasolid_topology_attribute_class_uses(
            std::slice::from_ref(&reference),
            std::slice::from_ref(&entity),
            &[instance_uses[0].clone(), instance_uses[0].clone()],
        )
        .is_empty());

        let mut invalid = entity;
        invalid.definition_xmt = 33;
        assert!(super::parasolid_attribute_class_uses(
            std::slice::from_ref(&invalid),
            std::slice::from_ref(&definition),
        )
        .is_empty());
        assert!(super::parasolid_topology_attribute_class_uses(
            &[reference],
            std::slice::from_ref(&invalid),
            &super::parasolid_attribute_class_uses(&[invalid.clone()], &[definition]),
        )
        .is_empty());
    }

    #[test]
    fn topology_attribute_class_uses_follow_type_81_owner_references() {
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 0,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(20).unwrap(),
            next_definition_xmt: None,
            identifier_xmt: crate::framing::xmt_reference::NonNullXmt::try_from(21).unwrap(),
            identifier_inflated_offset: 10,
            name: crate::printable_string::PrintableString::new("CLASS".to_string()).unwrap(),
            type_id: std::num::NonZeroU32::new(8000).unwrap(),
            action_codes: [AttributeAction::Code0; 8],
            field_names_xmt: None,
            legal_owner_flags: crate::parasolid::LegalOwnerFlags::Sixteen([false; 16]),

            field_codes: vec![AttributeField::Integer],
            inflated_offset: 20,
        };
        let head = ParasolidEntity51Record {
            id: "head".into(),
            stream_ordinal: 0,
            xmt: NonNullXmt::try_from(30).unwrap(),
            sequence: NonZeroU32::new(1).unwrap(),
            definition_xmt: 20,
            leading_references: [40, 1, 1, 1, 1],
            trailing_references: EntityReferences::new(vec![50]).unwrap(),
            byte_len: 26,
            inflated_offset: 30,
        };
        let child = ParasolidEntity51Record {
            id: "child".into(),
            xmt: NonNullXmt::try_from(31).unwrap(),
            sequence: NonZeroU32::new(2).unwrap(),
            leading_references: [40, 1, 999, 1, 1],
            inflated_offset: 60,
            ..head.clone()
        };
        let reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 0,
            topology_type: TopologyAttributeKind::Face,
            topology_xmt: 40,
            attribute_list_xmt: 30,
            attribute_list_record: Some(head.id.clone()),
            inflated_offset: 80,
        };
        let class_uses = super::parasolid_attribute_class_uses(
            &[head.clone(), child.clone()],
            std::slice::from_ref(&definition),
        );

        let uses = super::parasolid_topology_attribute_class_uses(
            std::slice::from_ref(&reference),
            &[head, child],
            &class_uses,
        );

        assert_eq!(uses.len(), 2);
        assert!(uses.iter().any(|use_| use_.entity_51_record == "head"));
        assert!(uses.iter().any(|use_| use_.entity_51_record == "child"));
        assert!(uses.iter().any(|use_| use_.id.ends_with("-31")));
    }

    #[test]
    fn entity_51_value_uses_exclude_fixed_leading_references() {
        use super::{
            ParasolidEntity51Record, ParasolidEntity52IntegerRecord, ParasolidEntity54StringRecord,
        };

        let entity = ParasolidEntity51Record {
            id: "entity".into(),
            stream_ordinal: 3,
            xmt: NonNullXmt::try_from(50).unwrap(),
            sequence: NonZeroU32::new(7).unwrap(),
            definition_xmt: 34,
            leading_references: [60, 61, 70, 71, 72],
            trailing_references: EntityReferences::new(vec![70, 71]).unwrap(),
            byte_len: 28,
            inflated_offset: 200,
        };
        let integers = [ParasolidEntity52IntegerRecord {
            id: "integers".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(70).unwrap(),
            values: crate::parasolid::counted_values::CountedValues::new(vec![1]).unwrap(),
            byte_len: 12,
            inflated_offset: 300,
        }];
        let strings = [ParasolidEntity54StringRecord {
            id: "string".into(),
            stream_ordinal: 3,
            xmt: crate::framing::xmt_reference::NonNullXmt::try_from(71).unwrap(),
            value: PrintableString::new("value".to_owned()).unwrap(),
            byte_len: 14,
            inflated_offset: 400,
        }];

        let numeric_uses =
            super::parasolid_entity_51_numeric_uses(std::slice::from_ref(&entity), &integers, &[]);
        assert_eq!(numeric_uses.len(), 1);
        assert_eq!(numeric_uses[0].position.reference_ordinal(), 5);
        assert_eq!(u32::from(numeric_uses[0].referenced_xmt), 70);

        let string_uses =
            super::parasolid_entity_51_string_uses(std::slice::from_ref(&entity), &strings);
        assert_eq!(string_uses.len(), 1);
        assert_eq!(string_uses[0].position.reference_ordinal(), 6);
        assert_eq!(u32::from(string_uses[0].referenced_xmt), 71);
    }
    mod attribute_wire;
    mod carrier_and_attribute_resolution;
}
